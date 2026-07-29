use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Style {
    pub id: String,
    /// Instruction appended to the polish prompt, e.g.
    /// "Formal tone: no slang, full sentences, polite forms."
    pub instruction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleSettings {
    /// Style id applied when no per-app override matches. Empty = neutral.
    pub default: String,
    /// Bundle id -> style id.
    pub per_app: HashMap<String, String>,
    pub styles: Vec<Style>,
}

impl Default for StyleSettings {
    fn default() -> Self {
        Self {
            default: String::new(),
            per_app: HashMap::new(),
            styles: vec![
                Style {
                    id: "formal".into(),
                    instruction: "Formal tone: полные предложения, вежливые формы, без сленга и \
                                  разговорных сокращений."
                        .into(),
                },
                Style {
                    id: "casual".into(),
                    instruction: "Casual tone: разговорный стиль, коротко и живо, уместны \
                                  сокращения; без канцелярита."
                        .into(),
                },
            ],
        }
    }
}

impl StyleSettings {
    /// Resolve the style instruction for the given frontmost app.
    pub fn instruction_for(&self, bundle_id: Option<&str>) -> Option<&str> {
        let id = bundle_id
            .and_then(|b| self.per_app.get(b))
            .unwrap_or(&self.default);
        if id.is_empty() {
            return None;
        }
        self.styles
            .iter()
            .find(|s| &s.id == id)
            .map(|s| s.instruction.as_str())
    }
}
