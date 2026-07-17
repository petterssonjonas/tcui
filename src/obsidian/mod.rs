#![allow(dead_code)]
pub mod safety;
pub mod vault;

pub use vault::Vault;

#[derive(Debug, Clone)]
pub struct Diff {
    pub path: String,
    pub old_content: String,
    pub new_content: String,
}

impl Diff {
    pub fn new(path: String, old_content: String, new_content: String) -> Self {
        Self {
            path,
            old_content,
            new_content,
        }
    }
}
