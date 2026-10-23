use serde::{Deserialize, Serialize};

use super::store::MemoryError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryScope {
    #[default]
    Unspecified,
    User,
    Machine,
}

impl MemoryScope {
    pub(super) fn cache_value(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::User => "user",
            Self::Machine => "machine",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MemoryMachine {
    pub(crate) id: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MemorySource {
    pub(crate) conversation_id: Option<i64>,
    pub(crate) conversation_title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct MemoryMetadata {
    pub(crate) scope: MemoryScope,
    pub(crate) machine: Option<MemoryMachine>,
    pub(crate) tags: Vec<String>,
    pub(crate) source: Option<MemorySource>,
}

impl MemoryMetadata {
    pub(super) fn validate(&self) -> Result<(), MemoryError> {
        match (self.scope, self.machine.as_ref()) {
            (MemoryScope::Machine, Some(machine))
                if !machine.id.trim().is_empty() && !machine.name.trim().is_empty() =>
            {
                Ok(())
            }
            (MemoryScope::Machine, _) => Err(MemoryError::Invalid(
                "machine-scoped memory requires a machine ID and name".to_string(),
            )),
            (_, Some(_)) => Err(MemoryError::Invalid(
                "only machine-scoped memory may carry machine metadata".to_string(),
            )),
            (_, None) => Ok(()),
        }
    }

    pub(super) fn same_scope(&self, other: &Self) -> bool {
        self.scope == other.scope
            && self.machine.as_ref().map(|machine| &machine.id)
                == other.machine.as_ref().map(|machine| &machine.id)
    }
}

pub(super) fn machine_cache_key(id: &str) -> String {
    blake3::hash(id.as_bytes()).to_hex().to_string()
}
