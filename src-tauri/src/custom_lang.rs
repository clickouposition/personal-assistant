//! [custom] Deterministic language routing for post-processing.
//!
//! Small local models (e.g. Gemma via Ollama) are unreliable at deciding the
//! output language on their own: they tend to answer in English. So before the
//! LLM call we detect, in code:
//!   1. a spoken translation cue at the start of the transcript
//!      ("traduza para o inglês", "em português", "translate", ...), and
//!   2. the language the person actually spoke (simple word/accent heuristic),
//! and hand the model one explicit instruction through the
//! `${language_directive}` placeholder of the prompt. The cue itself is removed
//! from the text the model sees.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Pt,
    En,
}

impl Lang {
    fn name(self) -> &'static str {
        match self {
            Lang::Pt => "Brazilian Portuguese",
            Lang::En => "casual American English",
        }
    }
    fn other(self) -> Lang {
        match self {
            Lang::Pt => Lang::En,
            Lang::En => Lang::Pt,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Lang(Lang),
    Opposite,
}

/// Cues, longest first so "traduza para o inglês" wins over "traduza".
/// Matched against the normalized start of the transcript.
const CUES: &[(&str, Target)] = &[
    ("traduza para o ingles", Target::Lang(Lang::En)),
    ("traduz para o ingles", Target::Lang(Lang::En)),
    ("traduza pro ingles", Target::Lang(Lang::En)),
    ("traduz pro ingles", Target::Lang(Lang::En)),
    ("traduza pra ingles", Target::Lang(Lang::En)),
    ("traduz pra ingles", Target::Lang(Lang::En)),
    ("traduza para ingles", Target::Lang(Lang::En)),
    ("traduz para ingles", Target::Lang(Lang::En)),
    ("translate to english", Target::Lang(Lang::En)),
    ("translate into english", Target::Lang(Lang::En)),
    ("traduza para o portugues", Target::Lang(Lang::Pt)),
    ("traduz para o portugues", Target::Lang(Lang::Pt)),
    ("traduza pro portugues", Target::Lang(Lang::Pt)),
    ("traduz pro portugues", Target::Lang(Lang::Pt)),
    ("traduza pra portugues", Target::Lang(Lang::Pt)),
    ("traduz pra portugues", Target::Lang(Lang::Pt)),
    ("traduza para portugues", Target::Lang(Lang::Pt)),
    ("traduz para portugues", Target::Lang(Lang::Pt)),
    ("translate to portuguese", Target::Lang(Lang::Pt)),
    ("translate into portuguese", Target::Lang(Lang::Pt)),
    ("em ingles", Target::Lang(Lang::En)),
    ("in english", Target::Lang(Lang::En)),
    ("em portugues", Target::Lang(Lang::Pt)),
    ("in portuguese", Target::Lang(Lang::Pt)),
    ("traduzir", Target::Opposite),
    ("traduza", Target::Opposite),
    ("traduz", Target::Opposite),
    ("translate", Target::Opposite),
];

/// Lowercase and strip accents so "Traduza para o Inglês" == "traduza para o ingles".
fn fold(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
        'ç' | 'Ç' => 'c',
        _ => c.to_ascii_lowercase(),
    }
}

/// Words of the text, folded, with their byte end offset in the original.
fn words_with_offsets(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            cur.push(fold(c));
        } else if !cur.is_empty() {
            out.push((std::mem::take(&mut cur), i));
        }
    }
    if !cur.is_empty() {
        out.push((cur, text.len()));
    }
    out
}

/// If the transcript starts with a cue, return it and the remaining text.
fn split_cue(text: &str) -> Option<(Target, &str)> {
    let words = words_with_offsets(text);
    for (cue, target) in CUES {
        let cue_words: Vec<&str> = cue.split(' ').collect();
        if words.len() < cue_words.len() {
            continue;
        }
        if words
            .iter()
            .zip(&cue_words)
            .all(|((w, _), c)| w.as_str() == *c)
        {
            let end = words[cue_words.len() - 1].1;
            let rest = text[end..].trim_start_matches(|c: char| {
                c.is_whitespace() || matches!(c, '.' | ',' | ':' | ';' | '!' | '-' | '—')
            });
            return Some((*target, rest));
        }
    }
    None
}

const PT_WORDS: &[&str] = &[
    "que", "nao", "de", "do", "da", "pra", "para", "voce", "voces", "eu", "e", "um", "uma", "com",
    "no", "na", "os", "as", "ta", "to", "tem", "vou", "vai", "pessoal", "galera", "oi", "obrigado",
    "valeu", "isso", "esse", "essa", "mas", "meu", "minha", "ele", "ela", "a", "o", "se", "por",
    "agora", "hoje", "amanha", "ontem", "alguem", "consegue", "quebrou", "mexeu", "tudo", "bem",
];
const EN_WORDS: &[&str] = &[
    "the", "you", "i", "is", "are", "can", "to", "and", "it", "my", "your", "we", "this", "that",
    "for", "with", "have", "will", "what", "hey", "guys", "please", "thanks", "of", "in", "on",
    "be", "do", "dont", "im", "its", "just", "get", "me", "send", "when", "how", "did", "was",
];

/// Guess the spoken language. Portuguese accents and common words vs English ones.
pub fn detect(text: &str) -> Lang {
    let accents = text
        .chars()
        .filter(|c| "ãõçáéíóúâêôàÃÕÇÁÉÍÓÚÂÊÔÀ".contains(*c))
        .count();
    let (mut pt, mut en) = (accents * 2, 0usize);
    for (w, _) in words_with_offsets(text) {
        if PT_WORDS.contains(&w.as_str()) {
            pt += 1;
        }
        if EN_WORDS.contains(&w.as_str()) {
            en += 1;
        }
    }
    if en > pt {
        Lang::En
    } else {
        Lang::Pt
    }
}

/// Returns (text for the model, directive for `${language_directive}`).
pub fn prepare(transcription: &str) -> (String, String) {
    let (target, text) = match split_cue(transcription) {
        Some((target, rest)) if !rest.trim().is_empty() => (Some(target), rest),
        _ => (None, transcription),
    };
    let source = detect(text);
    let out = match target {
        None => source,
        Some(Target::Opposite) => source.other(),
        Some(Target::Lang(l)) => l,
    };
    let directive = if out == source {
        format!(
            "The message is in {src}. Write the final message in {src}. Do NOT translate it.",
            src = source.name()
        )
    } else {
        format!(
            "The message is in {src}. Translate it into {dst}: translate the meaning, not word by word, the way a native speaker would naturally say it.",
            src = source.name(),
            dst = out.name()
        )
    };
    (text.to_string(), directive)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &str) -> (String, String) {
        prepare(t)
    }

    #[test]
    fn portuguese_without_cue_stays_portuguese() {
        let (text, d) = run("Galera, o build quebrou de novo, alguém mexeu na pipeline?");
        assert!(text.starts_with("Galera"));
        assert!(d.contains("Do NOT translate") && d.contains("Portuguese"));
    }

    #[test]
    fn english_without_cue_stays_english() {
        let (_, d) = run("Hey guys, can someone review my PR?");
        assert!(d.contains("Do NOT translate") && d.contains("English"));
    }

    #[test]
    fn explicit_cue_to_english_is_removed() {
        let (text, d) = run("Traduza para o inglês. Pessoal, subi a correção do login");
        assert_eq!(text, "Pessoal, subi a correção do login");
        assert!(d.contains("into casual American English"));
    }

    #[test]
    fn explicit_cue_to_portuguese() {
        let (text, d) = run("Traduza para o português: thanks for the help yesterday");
        assert_eq!(text, "thanks for the help yesterday");
        assert!(d.contains("into Brazilian Portuguese"));
    }

    #[test]
    fn bare_cue_flips_language() {
        let (text, d) = run("Traduz. Can you send me the slides when you get a chance?");
        assert_eq!(text, "Can you send me the slides when you get a chance?");
        assert!(d.contains("into Brazilian Portuguese"));
        let (_, d) = run("Traduz, tô morrendo de fome, bora almoçar");
        assert!(d.contains("into casual American English"));
    }

    #[test]
    fn cue_only_is_not_stripped_to_empty() {
        let (text, _) = run("Traduz");
        assert_eq!(text, "Traduz");
    }
}
