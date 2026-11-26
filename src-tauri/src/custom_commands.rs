//! [custom] Voice commands: a transcript that starts with the wake word
//! ("Computador, ..." / "Computer, ...") is not pasted. It is handed to
//! `<app data dir>/commands/vocal_commands.py`, which asks the local LLM to pick
//! ONE action from a fixed whitelist and runs it. Nothing free-form is executed.

use log::{debug, error, info};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tauri::AppHandle;
use tokio::process::Command;

const WAKE_WORDS: &[&str] = &["computador", "computer"];
const COMMAND_TIMEOUT: Duration = Duration::from_secs(25);

/// If the transcript starts with the wake word, return the command after it.
pub fn command_text(transcript: &str) -> Option<String> {
    let trimmed = transcript.trim_start();
    let end = trimmed
        .char_indices()
        .find(|(_, c)| !c.is_alphanumeric())
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    let first = trimmed[..end].to_lowercase();
    if !WAKE_WORDS.contains(&first.as_str()) {
        return None;
    }
    let rest = trimmed[end..]
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | ':' | '!' | '-'))
        .trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

fn script_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = crate::portable::app_data_dir(app).ok()?;
    let script = dir.join("commands").join("vocal_commands.py");
    script.is_file().then_some(script)
}

/// Run the command script. Never pastes anything; errors only go to the log
/// (the script itself shows a macOS notification with the result).
pub async fn run(app: &AppHandle, command: &str) {
    let Some(script) = script_path(app) else {
        error!("Voice command ignored: commands/vocal_commands.py not installed");
        return;
    };
    info!("Voice command: running {}", script.display());
    let child = Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(&script)
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let child = match child {
        Ok(c) => c,
        Err(e) => {
            error!("Voice command failed to start: {}", e);
            return;
        }
    };
    match tokio::time::timeout(COMMAND_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(out)) => {
            debug!(
                "Voice command exit={:?} stdout={} stderr={}",
                out.status.code(),
                String::from_utf8_lossy(&out.stdout).trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(Err(e)) => error!("Voice command error: {}", e),
        Err(_) => error!("Voice command timed out after {:?}", COMMAND_TIMEOUT),
    }
}

#[cfg(test)]
mod tests {
    use super::command_text;

    #[test]
    fn wake_word_is_detected_and_stripped() {
        assert_eq!(
            command_text("Computador, abre o Slack no grupo Marketing.").as_deref(),
            Some("abre o Slack no grupo Marketing.")
        );
        assert_eq!(
            command_text("computer close all Chrome tabs").as_deref(),
            Some("close all Chrome tabs")
        );
    }

    #[test]
    fn normal_dictation_is_not_a_command() {
        assert_eq!(command_text("O computador travou de novo"), None);
        assert_eq!(command_text("Computadores novos chegaram"), None);
        assert_eq!(command_text("Computador."), None);
    }
}
