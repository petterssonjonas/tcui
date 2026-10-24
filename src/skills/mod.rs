mod builtins;
mod catalog;
mod frontmatter;
mod mentions;

use std::borrow::Cow;
use std::path::PathBuf;

pub use catalog::SkillCatalog;
pub use mentions::mentions;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SkillOrigin {
    Builtin,
    External(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub origin: SkillOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub source: Cow<'static, str>,
    pub origin: SkillOrigin,
}
