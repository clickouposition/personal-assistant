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

/// Is this (folded) word a "translate" verb? Prefix match tolerates
/// Whisper variants: traduz/traduza/traduzir/traduzido, translate/translated/
/// translation/translating.
fn is_translate_verb(w: &str) -> bool {
    w.starts_with("traduz") || w.starts_with("translat")
}

/// Language named by a (folded) word, tolerating misspellings
/// ("portugues", "portuguese", "portuguesa", "ingles", "english").
fn lang_word(w: &str) -> Option<Lang> {
    if w.starts_with("portug") {
        Some(Lang::Pt)
    } else if w.starts_with("ingl") || w.starts_with("engl") {
        Some(Lang::En)
    } else {
        None
    }
}

/// Small words allowed inside a cue between the verb and the language:
/// "traduza [isso] para o inglês", "translate [this] [from English] to Portuguese".
const CUE_FILLERS: &[&str] = &[
    "para", "pra", "pro", "p", "o", "a", "os", "as", "do", "da", "de", "em", "no", "na", "isso",
    "isto", "aqui", "por", "favor", "texto", "mensagem", "lingua", "idioma", "brasileiro",
    "americano", "to", "into", "in", "the", "this", "it", "from", "please", "text", "message",
    "language", "brazilian", "american", "and",
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

/// Words of the text, folded, with their byte (start, end) offsets in the original.
fn words_with_offsets(text: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            if cur.is_empty() {
                start = i;
            }
            cur.push(fold(c));
        } else if !cur.is_empty() {
            out.push((std::mem::take(&mut cur), start, i));
        }
    }
    if !cur.is_empty() {
        out.push((cur, start, text.len()));
    }
    out
}

/// Try to read a cue starting at word `i`. Returns the target and the index
/// of the last word that belongs to the cue.
///
/// Forms: "<verb> [fillers] [<lang> ...]" (the LAST language named wins, so
/// "traduza do inglês para o português" -> Portuguese) and "em|in <lang>".
fn parse_cue(words: &[(String, usize, usize)], i: usize) -> Option<(Target, usize)> {
    let w = words.get(i)?.0.as_str();
    if (w == "em" || w == "in") && words.get(i + 1).and_then(|n| lang_word(&n.0)).is_some() {
        return Some((Target::Lang(lang_word(&words[i + 1].0)?), i + 1));
    }
    if !is_translate_verb(w) {
        return None;
    }
    let (mut target, mut last) = (Target::Opposite, i);
    for (j, (word, _, _)) in words.iter().enumerate().skip(i + 1).take(8) {
        if let Some(l) = lang_word(word) {
            target = Target::Lang(l);
            last = j;
        } else if !CUE_FILLERS.contains(&word.as_str()) {
            break;
        }
    }
    Some((target, last))
}

const PUNCT: &[char] = &['.', ',', ':', ';', '!', '-', '—', '?'];

/// Find a translation cue at the start (optionally after "ok"/"please") or at
/// the end of the transcript ("..., translate to Portuguese"). Returns the
/// target and the transcript without the cue.
fn split_cue(text: &str) -> Option<(Target, String)> {
    let words = words_with_offsets(text);
    if words.is_empty() {
        return None;
    }
    // Cue at the start.
    let first = if matches!(words[0].0.as_str(), "ok" | "okay" | "please" | "hey") { 1 } else { 0 };
    if let Some((target, last)) = parse_cue(&words, first) {
        let rest = text[words[last].2..]
            .trim_start_matches(|c: char| c.is_whitespace() || PUNCT.contains(&c));
        return Some((target, rest.to_string()));
    }
    // Cue at the end: only the verb form (a sentence may naturally end in
    // "in English"), and it must run to the last word.
    let n = words.len();
    for k in n.saturating_sub(9)..n {
        if !is_translate_verb(&words[k].0) {
            continue;
        }
        if let Some((target, last)) = parse_cue(&words, k) {
            let trailing_ok = words[last + 1..]
                .iter()
                .all(|(w, _, _)| CUE_FILLERS.contains(&w.as_str()));
            if trailing_ok && k > 0 {
                let rest = text[..words[k].1]
                    .trim_end_matches(|c: char| c.is_whitespace() || PUNCT.contains(&c));
                return Some((target, rest.to_string()));
            }
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
    for (w, _, _) in words_with_offsets(text) {
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
        _ => (None, transcription.to_string()),
    };
    let source = detect(&text);
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
    (text, directive)
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

    #[test]
    fn whisper_variant_translated_to_portuguese() {
        // Real transcript: Whisper heard "translated" instead of "translate".
        let (text, d) = run("translated to Portuguese hey baby how are you you are the love of my life");
        assert_eq!(text, "hey baby how are you you are the love of my life");
        assert!(d.contains("into Brazilian Portuguese"));
    }

    #[test]
    fn from_x_to_y_uses_last_language() {
        let (text, d) = run("Traduza do inglês para o português. I'll send the report tomorrow");
        assert_eq!(text, "I'll send the report tomorrow");
        assert!(d.contains("into Brazilian Portuguese"));
        let (_, d) = run("traduza do português para o inglês. Oi, meu amor, tudo bem?");
        assert!(d.contains("into casual American English"));
        let (_, d) = run("Translate this from English to Portuguese: see you tomorrow");
        assert!(d.contains("into Brazilian Portuguese"));
    }

    #[test]
    fn misspelled_language_and_please() {
        let (_, d) = run("Please translate to portugues, the build is broken again");
        assert!(d.contains("into Brazilian Portuguese"));
        let (_, d) = run("Traduzir pro inglês: a reunião foi cancelada");
        assert!(d.contains("into casual American English"));
    }

    #[test]
    fn cue_at_the_end() {
        let (text, d) = run("Hey, can you send me the slides later? Translate to Portuguese.");
        assert_eq!(text, "Hey, can you send me the slides later");
        assert!(d.contains("into Brazilian Portuguese"));
        let (text, d) = run("A reunião foi cancelada, traduz pro inglês");
        assert_eq!(text, "A reunião foi cancelada");
        assert!(d.contains("into casual American English"));
    }

    #[test]
    fn sentence_ending_in_language_name_is_not_a_cue() {
        let (text, d) = run("I wrote the whole doc in English");
        assert_eq!(text, "I wrote the whole doc in English");
        assert!(d.contains("Do NOT translate"));
    }
}
