/// Builds the whisper `initial_prompt` from user dictionary terms.
///
/// Empirically (tested on real RU+EN code-switched dictation) a seed *sentence*
/// mixing Russian with Latin-spelled tech terms primes whisper much better than
/// a bare glossary list: it both keeps English terms in Latin script and rescues
/// short Russian words like "на прод" from being misheard.
const SEED: &str = "Запушь коммит в репозиторий на GitHub, задеплой на прод и открой pull request.";

/// Rough cap so the prompt never eats a significant part of whisper's text context.
const MAX_PROMPT_CHARS: usize = 600;

pub fn build_initial_prompt(terms: &[String]) -> String {
    let mut prompt = String::from(SEED);
    if !terms.is_empty() {
        prompt.push_str(" Термины: ");
        let mut first = true;
        for term in terms {
            let addition = if first {
                term.clone()
            } else {
                format!(", {term}")
            };
            if prompt.len() + addition.len() + 1 > MAX_PROMPT_CHARS {
                break;
            }
            prompt.push_str(&addition);
            first = false;
        }
        prompt.push('.');
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_terms_gives_seed_only() {
        assert_eq!(build_initial_prompt(&[]), SEED);
    }

    #[test]
    fn terms_are_appended() {
        let terms = vec!["Kubernetes".to_string(), "Tauri".to_string()];
        let p = build_initial_prompt(&terms);
        assert!(p.contains("Kubernetes, Tauri."));
    }

    #[test]
    fn prompt_is_capped() {
        let terms: Vec<String> = (0..200).map(|i| format!("term{i}")).collect();
        let p = build_initial_prompt(&terms);
        assert!(p.len() <= MAX_PROMPT_CHARS + 10);
    }
}
