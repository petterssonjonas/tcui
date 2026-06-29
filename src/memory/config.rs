use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct MemoryConfig {
    pub(crate) enabled: bool,
    pub(crate) auto_capture: bool,
    pub(crate) max_memories: u8,
    pub(crate) max_context_chars: usize,
    pub(crate) min_similarity: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_capture: true,
            max_memories: 2,
            max_context_chars: 320,
            min_similarity: 0.55,
        }
    }
}
