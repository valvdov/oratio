pub mod ollama;
pub mod openai_compat;
pub mod prompt;
pub mod regex_clean;

use crate::Result;

#[derive(Debug, Clone)]
pub struct PolishRequest<'a> {
    pub raw: &'a str,
    /// Extra style instruction, e.g. "Formal tone for email."
    pub style: Option<&'a str>,
    /// Dictionary terms whose exact spelling must be preserved/restored.
    pub dictionary: &'a [String],
}

pub trait PolishProvider: Send + Sync {
    fn name(&self) -> &str;
    fn polish(&self, req: &PolishRequest) -> Result<String>;
}

/// Sanity filter on LLM output: reject obviously broken responses so the
/// caller falls back to regex cleanup instead of inserting garbage.
pub fn plausible_output(raw: &str, polished: &str) -> bool {
    let polished = polished.trim();
    if polished.is_empty() {
        return false;
    }
    let raw_len = raw.chars().count() as f32;
    let out_len = polished.chars().count() as f32;
    // Polished text should be shorter-or-similar; wildly longer means the
    // model answered the text instead of editing it.
    out_len <= raw_len * 1.6 + 40.0 && out_len >= raw_len * 0.2
}
