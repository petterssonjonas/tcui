use std::collections::{HashMap, HashSet};

use crate::config::vault_profiles::{normalize_name, VaultProfile, VaultProfileId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultMention {
    Vault,
    Obsidian,
}

impl VaultMention {
    const fn token(self) -> &'static str {
        match self {
            Self::Vault => "@vault",
            Self::Obsidian => "@obsidian",
        }
    }

    pub(crate) fn first_in(request: &str) -> Option<Self> {
        match (request.find("@vault"), request.find("@obsidian")) {
            (Some(vault), Some(obsidian)) if vault <= obsidian => Some(Self::Vault),
            (Some(_), Some(_)) => Some(Self::Obsidian),
            (Some(_), None) => Some(Self::Vault),
            (None, Some(_)) => Some(Self::Obsidian),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VaultTarget {
    pub(crate) profile_id: VaultProfileId,
    pub(crate) display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum VaultTargetError {
    #[error("unknown explicit Markdown Vault target '{value}'; valid names: {valid_names:?}")]
    UnknownExplicit {
        value: String,
        valid_names: Vec<String>,
    },
    #[error("Markdown Vault target is ambiguous ({candidates:?}); valid names: {valid_names:?}")]
    Ambiguous {
        candidates: Vec<String>,
        valid_names: Vec<String>,
    },
    #[error("Markdown Vault has no default profile; valid names: {valid_names:?}")]
    MissingDefault { valid_names: Vec<String> },
    #[error("invalid explicit Markdown Vault target; valid names: {valid_names:?}")]
    InvalidExplicit { valid_names: Vec<String> },
}

pub(crate) struct VaultTargetResolver<'a> {
    profiles: &'a HashMap<VaultProfileId, VaultProfile>,
    default_id: Option<&'a VaultProfileId>,
}

impl<'a> VaultTargetResolver<'a> {
    pub(crate) const fn new(
        profiles: &'a HashMap<VaultProfileId, VaultProfile>,
        default_id: Option<&'a VaultProfileId>,
    ) -> Self {
        Self {
            profiles,
            default_id,
        }
    }

    pub(crate) fn resolve(
        &self,
        request: &str,
        mention: VaultMention,
    ) -> Result<VaultTarget, VaultTargetError> {
        let valid_names = self.allowed_names();
        let invalid_explicit = || VaultTargetError::InvalidExplicit {
            valid_names: valid_names.clone(),
        };
        let mut explicit_values =
            immediate_quoted_values(request, mention.token()).map_err(|_| invalid_explicit())?;
        explicit_values.extend(marker_values(request).map_err(|_| invalid_explicit())?);
        if !explicit_values.is_empty() {
            return self.resolve_explicit(&explicit_values, valid_names);
        }

        let suffix = mention_suffix(request, mention.token()).unwrap_or_default();
        let mut prefix_matches = self
            .profiles
            .values()
            .filter(|profile| normalized_prefix_matches(suffix, profile))
            .collect::<Vec<_>>();
        prefix_matches.sort_by_key(|profile| std::cmp::Reverse(profile.name_key.as_str().len()));
        if let Some(profile) = prefix_matches.first() {
            return Ok(target(profile));
        }

        let mention_search = request
            .replace(VaultMention::Vault.token(), " ")
            .replace(VaultMention::Obsidian.token(), " ");
        let normalized_request = normalize_name(&mention_search);
        let mentioned = self
            .profiles
            .values()
            .filter(|profile| contains_name(normalized_request.as_str(), profile.name_key.as_str()))
            .collect::<Vec<_>>();
        match mentioned.as_slice() {
            [profile] => Ok(target(profile)),
            [] => self.default_target(valid_names),
            candidates => Err(VaultTargetError::Ambiguous {
                candidates: sorted_names(candidates.iter().copied()),
                valid_names,
            }),
        }
    }

    fn resolve_explicit(
        &self,
        values: &[String],
        valid_names: Vec<String>,
    ) -> Result<VaultTarget, VaultTargetError> {
        let mut matches = Vec::new();
        let mut seen = HashSet::new();
        for value in values {
            let key = normalize_name(value);
            let Some(profile) = self
                .profiles
                .values()
                .find(|profile| profile.name_key == key)
            else {
                return Err(VaultTargetError::UnknownExplicit {
                    value: value.clone(),
                    valid_names,
                });
            };
            if seen.insert(&profile.id) {
                matches.push(profile);
            }
        }
        match matches.as_slice() {
            [profile] => Ok(target(profile)),
            candidates => Err(VaultTargetError::Ambiguous {
                candidates: sorted_names(candidates.iter().copied()),
                valid_names,
            }),
        }
    }

    fn default_target(&self, valid_names: Vec<String>) -> Result<VaultTarget, VaultTargetError> {
        self.default_id
            .and_then(|id| self.profiles.get(id))
            .map(target)
            .ok_or(VaultTargetError::MissingDefault { valid_names })
    }

    pub(crate) fn allowed_names(&self) -> Vec<String> {
        sorted_names(self.profiles.values())
    }
}

fn target(profile: &VaultProfile) -> VaultTarget {
    VaultTarget {
        profile_id: profile.id.clone(),
        display_name: profile.display_name.clone(),
    }
}

fn sorted_names<'a>(profiles: impl Iterator<Item = &'a VaultProfile>) -> Vec<String> {
    let mut names = profiles
        .map(|profile| profile.display_name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn mention_suffix<'a>(request: &'a str, token: &str) -> Option<&'a str> {
    request
        .find(token)
        .map(|index| request[index + token.len()..].trim_start_matches(char::is_whitespace))
}

fn immediate_quoted_values(request: &str, token: &str) -> Result<Vec<String>, VaultTargetError> {
    let Some(suffix) = mention_suffix(request, token) else {
        return Ok(Vec::new());
    };
    if !suffix.starts_with('"') {
        return Ok(Vec::new());
    }
    quoted_value(suffix)
        .map(|value| vec![value])
        .ok_or_else(|| VaultTargetError::InvalidExplicit {
            valid_names: Vec::new(),
        })
}

fn marker_values(request: &str) -> Result<Vec<String>, VaultTargetError> {
    let bytes = request.as_bytes();
    let mut values = Vec::new();
    let mut index = 0;
    while index + 6 <= bytes.len() {
        let marker = &bytes[index..index + 6];
        let left_valid =
            index == 0 || (!is_name_byte(bytes[index - 1]) && bytes[index - 1] != b'@');
        if left_valid
            && marker[..5].eq_ignore_ascii_case(b"vault")
            && matches!(marker[5], b':' | b'=')
        {
            let suffix = &request[index + 6..];
            let value = if suffix.starts_with('"') {
                quoted_value(suffix)
            } else {
                suffix.split_whitespace().next().map(str::to_owned)
            };
            let Some(value) = value.filter(|value| !value.is_empty()) else {
                return Err(VaultTargetError::InvalidExplicit {
                    valid_names: Vec::new(),
                });
            };
            index += 6 + value.len();
            values.push(value);
            continue;
        }
        index += 1;
    }
    Ok(values)
}

fn quoted_value(value: &str) -> Option<String> {
    let remainder = value.strip_prefix('"')?;
    let end = remainder.find('"')?;
    Some(remainder[..end].to_owned())
}

fn normalized_prefix_matches(suffix: &str, profile: &VaultProfile) -> bool {
    let normalized = normalize_name(suffix);
    normalized
        .as_str()
        .strip_prefix(profile.name_key.as_str())
        .is_some_and(|rest| {
            rest.chars()
                .next()
                .is_none_or(|character| !character.is_alphanumeric())
        })
}

fn contains_name(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(index, matched)| {
        let left = haystack[..index].chars().next_back();
        let right = haystack[index + matched.len()..].chars().next();
        left.is_none_or(|character| !character.is_alphanumeric())
            && right.is_none_or(|character| !character.is_alphanumeric())
    })
}

const fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}
