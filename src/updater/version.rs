use color_eyre::{eyre::eyre, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

pub(super) fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches('v').to_string()
}

pub(super) fn parse_version(version: &str) -> Result<Version> {
    let version = normalize_version(version);
    let core = version.split('-').next().unwrap_or_default();
    let mut parts = core.split('.');
    let major = parts
        .next()
        .ok_or_else(|| eyre!("Missing major version in {}", version))?
        .parse()?;
    let minor = parts
        .next()
        .ok_or_else(|| eyre!("Missing minor version in {}", version))?
        .parse()?;
    let patch = parts
        .next()
        .ok_or_else(|| eyre!("Missing patch version in {}", version))?
        .parse()?;
    Ok(Version {
        major,
        minor,
        patch,
    })
}

#[cfg(test)]
mod tests {
    use super::{normalize_version, parse_version};

    #[test]
    fn normalizes_v_prefixed_tags() {
        assert_eq!(normalize_version("v0.6.0"), "0.6.0");
        assert_eq!(normalize_version("0.6.0"), "0.6.0");
    }

    #[test]
    fn parses_semver_components_for_comparison() {
        assert!(
            parse_version("0.7.0").expect("version") > parse_version("0.6.9").expect("version")
        );
    }
}
