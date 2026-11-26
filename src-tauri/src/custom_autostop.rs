//! [custom] Auto-stop for hands-free recordings.
//!
//! When a recording is locked on (the shortcut was tapped, not held), Handy
//! normally waits for a second press. Here, once the person has spoken and then
//! stays silent for a moment, the recording stops by itself, exactly as if the
//! shortcut had been pressed again. Held (push-to-talk) recordings are never
//! touched: releasing the key already ends them.

use log::debug;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

/// Silence needed after speech, on top of the VAD's own ~450 ms hangover tail.
pub const SILENCE_MS: u32 = 1000;
/// Ignore coughs and clicks: at least this much speech before auto-stop arms.
pub const MIN_SPEECH_MS: u32 = 500;

static APP: OnceLock<AppHandle> = OnceLock::new();
/// The binding of the current recording while it is locked on, else `None`.
static LOCKED_BINDING: Mutex<Option<String>> = Mutex::new(None);
/// Bumped every time a recording starts, so a late trigger from an older
/// recording can never stop a newer one.
static SESSION: AtomicU32 = AtomicU32::new(0);

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// Coordinator: a recording started (locked or not).
pub fn recording_started(binding_id: &str, locked: bool) {
    SESSION.fetch_add(1, Ordering::SeqCst);
    *LOCKED_BINDING.lock().unwrap() = locked.then(|| binding_id.to_string());
}

/// Coordinator: a tap locked the current recording on.
pub fn recording_locked(binding_id: &str) {
    *LOCKED_BINDING.lock().unwrap() = Some(binding_id.to_string());
}

/// Coordinator: recording ended (processing, cancel, idle).
pub fn recording_ended() {
    *LOCKED_BINDING.lock().unwrap() = None;
}

pub fn current_session() -> u32 {
    SESSION.load(Ordering::SeqCst)
}

/// Audio sink: speech followed by enough silence. Stops the recording if it is
/// still the same session and still locked on. Fires at most once per session.
pub fn end_of_speech(session: u32) {
    if session != current_session() {
        return;
    }
    let Some(binding) = LOCKED_BINDING.lock().unwrap().take() else {
        return;
    };
    let Some(app) = APP.get() else {
        return;
    };
    debug!("Auto-stop: silence after speech, stopping '{}'", binding);
    crate::signal_handle::send_transcription_input(app, &binding, "auto-stop");
}

/// Per-recording silence tracker used by the audio sink. Pure logic, no I/O.
#[derive(Debug, Default)]
pub struct SilenceTracker {
    speech_ms: u32,
    silence_ms: u32,
    fired: bool,
}

impl SilenceTracker {
    /// Feed one VAD frame. Returns true exactly once, when speech has been
    /// followed by `SILENCE_MS` of silence.
    pub fn push(&mut self, is_speech: bool, frame_ms: u32) -> bool {
        if self.fired {
            return false;
        }
        if is_speech {
            self.speech_ms += frame_ms;
            self.silence_ms = 0;
            return false;
        }
        if self.speech_ms < MIN_SPEECH_MS {
            return false;
        }
        self.silence_ms += frame_ms;
        if self.silence_ms >= SILENCE_MS {
            self.fired = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::SilenceTracker;

    #[test]
    fn fires_once_after_speech_then_silence() {
        let mut t = SilenceTracker::default();
        let mut fired: Vec<bool> = (0..20).map(|_| t.push(true, 32)).collect();
        fired.extend((0..40).map(|_| t.push(false, 32)));
        assert_eq!(fired.iter().filter(|f| **f).count(), 1);
        // 1000 ms / 32 ms -> fires on the 32nd silent frame.
        assert!(fired[20 + 31]);
    }

    #[test]
    fn silence_before_speech_never_fires() {
        let mut t = SilenceTracker::default();
        assert!(!(0..200).any(|_| t.push(false, 32)));
    }

    #[test]
    fn short_noise_does_not_arm() {
        let mut t = SilenceTracker::default();
        assert!(!(0..5).any(|_| t.push(true, 32))); // 160 ms "cough"
        assert!(!(0..100).any(|_| t.push(false, 32)));
    }

    #[test]
    fn a_pause_shorter_than_the_limit_keeps_recording() {
        let mut t = SilenceTracker::default();
        (0..20).for_each(|_| {
            t.push(true, 32);
        });
        assert!(!(0..20).any(|_| t.push(false, 32))); // 640 ms pause
        (0..10).for_each(|_| {
            t.push(true, 32);
        });
        assert!((0..40).any(|_| t.push(false, 32)));
    }
}
