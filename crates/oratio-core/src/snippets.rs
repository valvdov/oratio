use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    /// Spoken trigger phrase, e.g. "моя подпись".
    pub trigger: String,
    /// Text inserted verbatim when the trigger is dictated.
    pub expansion: String,
}

/// Normalize a transcript for trigger matching: lowercase, drop punctuation
/// and common fillers, collapse whitespace.
fn normalize(text: &str) -> String {
    const FILLERS: &[&str] = &["эээ", "ээ", "эм", "ммм", "ну", "um", "uh"];
    text.to_lowercase()
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .filter(|w| !w.is_empty() && !FILLERS.contains(&w.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Levenshtein distance on characters.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Find a snippet whose trigger matches the transcript (exact normalized match
/// or ≥ 0.85 similarity). Returns the expansion to insert verbatim.
pub fn match_snippet<'a>(transcript: &str, snippets: &'a [Snippet]) -> Option<&'a Snippet> {
    let normalized = normalize(transcript);
    if normalized.is_empty() {
        return None;
    }
    for snippet in snippets {
        let trigger = normalize(&snippet.trigger);
        if trigger.is_empty() {
            continue;
        }
        if normalized == trigger {
            return Some(snippet);
        }
        let max_len = normalized.chars().count().max(trigger.chars().count());
        let distance = levenshtein(&normalized, &trigger);
        let similarity = 1.0 - distance as f32 / max_len as f32;
        if similarity >= 0.85 {
            return Some(snippet);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snips() -> Vec<Snippet> {
        vec![Snippet {
            trigger: "моя подпись".into(),
            expansion: "С уважением,\nВалерий".into(),
        }]
    }

    #[test]
    fn exact_match() {
        assert!(match_snippet("Моя подпись.", &snips()).is_some());
    }

    #[test]
    fn match_with_filler() {
        assert!(match_snippet("моя эээ подпись", &snips()).is_some());
    }

    #[test]
    fn fuzzy_match() {
        assert!(match_snippet("мая подпись", &snips()).is_some());
    }

    #[test]
    fn no_match_on_longer_text() {
        assert!(match_snippet("вставь сюда мою подпись и отправь письмо", &snips()).is_none());
    }
}
