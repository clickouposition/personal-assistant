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

/// Wake word. Whisper may split or misspell it ("Mona Tela", "Monatella",
/// "Monotela"), so the first one or two words are compared with a small
/// edit-distance tolerance.
const WAKE_WORD: &str = "monatela";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(25);

fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'Á' | 'À' | 'Â' | 'Ã' => 'a',
            'é' | 'ê' | 'É' | 'Ê' => 'e',
            'í' | 'Í' => 'i',
            'ó' | 'ô' | 'õ' | 'Ó' | 'Ô' | 'Õ' => 'o',
            'ú' | 'Ú' => 'u',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

fn is_wake_word(candidate: &str) -> bool {
    candidate.starts_with("mon") && edit_distance(candidate, WAKE_WORD) <= 2
}

/// If the transcript starts with the wake word, return the command after it.
pub fn command_text(transcript: &str) -> Option<String> {
    // First words with their byte end offsets.
    let mut words: Vec<(String, usize)> = Vec::new();
    let mut cur = String::new();
    for (i, c) in transcript.char_indices() {
        if c.is_alphanumeric() {
            cur.push(c);
        } else if !cur.is_empty() {
            words.push((fold(&std::mem::take(&mut cur)), i));
            if words.len() == 2 {
                break;
            }
        }
    }
    if !cur.is_empty() && words.len() < 2 {
        words.push((fold(&cur), transcript.len()));
    }
    let end = match words.as_slice() {
        [(w1, e1), ..] if is_wake_word(w1) => *e1,
        // Split form ("Mona tela"): the second word must itself look like "tela".
        [(w1, _), (w2, e2), ..]
            if w2.starts_with("te") && w2.len() >= 4 && is_wake_word(&format!("{w1}{w2}")) =>
        {
            *e2
        }
        _ => return None,
    };
    let rest = transcript[end..]
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
        assert_eq!(command_text("Monatela, feche o WhatsApp.").as_deref(), Some("feche o WhatsApp."));
        assert_eq!(command_text("monatela close all Chrome tabs").as_deref(), Some("close all Chrome tabs"));
    }

    #[test]
    fn whisper_variants_are_accepted() {
        for t in ["Mona Tela, abre o Slack", "Monatella, abre o Slack", "Monotela abre o Slack", "Mônatela, abre o Slack", "Mona tela abre o Slack"] {
            assert_eq!(command_text(t).as_deref(), Some("abre o Slack"), "{t}");
        }
    }

    #[test]
    fn normal_dictation_is_not_a_command() {
        assert_eq!(command_text("Computador, feche o WhatsApp"), None);
        assert_eq!(command_text("Monitorar o servidor amanhã"), None);
        assert_eq!(command_text("Montei a tela nova do app"), None);
        assert_eq!(command_text("A Monatela fechou o contrato"), None);
        assert_eq!(command_text("Monatela."), None);
    }
}
