/// No-LLM fallback cleanup: strip common fillers, collapse whitespace,
/// capitalize, terminate the sentence. Deliberately conservative.

const FILLERS: &[&str] = &[
    "эээ", "ээ", "эм", "эмм", "ммм", "мм", "um", "uh", "uhm", "umm",
];

pub fn clean(raw: &str) -> String {
    let mut words: Vec<&str> = Vec::new();
    for word in raw.split_whitespace() {
        let bare: String = word
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        if FILLERS.contains(&bare.as_str()) {
            continue;
        }
        // Drop immediate duplicated words ("в в репозиторий").
        if let Some(prev) = words.last() {
            let prev_bare: String = prev
                .to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect();
            if !bare.is_empty() && bare == prev_bare {
                continue;
            }
        }
        words.push(word);
    }
    let mut text = words.join(" ");
    if let Some(first) = text.chars().next() {
        if first.is_lowercase() {
            let upper: String = first.to_uppercase().collect();
            text = format!("{upper}{}", &text[first.len_utf8()..]);
        }
    }
    let ends_with_punct = text
        .chars()
        .last()
        .map(|c| ".!?…:;".contains(c))
        .unwrap_or(true);
    if !ends_with_punct {
        text.push('.');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_fillers_and_duplicates() {
        assert_eq!(
            clean("эээ ну запушь в в репозиторий эм пожалуйста"),
            "Ну запушь в репозиторий пожалуйста."
        );
    }

    #[test]
    fn capitalizes_and_terminates() {
        assert_eq!(clean("привет мир"), "Привет мир.");
    }

    #[test]
    fn keeps_existing_punctuation() {
        assert_eq!(clean("Привет, мир!"), "Привет, мир!");
    }
}
