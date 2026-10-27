use color_eyre::Result;

pub(super) fn normalize_version(version: &str) -> String {
    let version = version.trim();
    version.strip_prefix('v').unwrap_or(version).to_owned()
}

pub(super) fn parse_version(version: &str) -> Result<semver::Version> {
    let version = version.trim();
    let mut parsed = semver::Version::parse(version.strip_prefix('v').unwrap_or(version))?;
    // Build metadata does not change SemVer precedence.
    parsed.build = semver::BuildMetadata::EMPTY;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{normalize_version, parse_version};

    #[test]
    fn normalizes_only_one_optional_v_prefix() {
        assert_eq!(normalize_version("v0.8.6"), "0.8.6");
        assert_eq!(normalize_version("0.8.6"), "0.8.6");
        assert!(parse_version("vv0.8.6").is_err());
    }

    #[test]
    fn compares_semantic_prereleases_and_build_metadata() {
        let ordered = [
            "0.8.5",
            "0.8.6-alpha",
            "0.8.6-alpha.1",
            "0.8.6-alpha.2",
            "0.8.6-alpha.10",
            "0.8.6-beta",
            "0.8.6-rc.1",
            "0.8.6",
            "0.8.7",
        ];
        for pair in ordered.windows(2) {
            assert!(parse_version(pair[0]).unwrap() < parse_version(pair[1]).unwrap());
        }
        assert_eq!(
            parse_version("v0.8.6+build.1").unwrap(),
            parse_version("0.8.6").unwrap()
        );
    }

    #[test]
    fn rejects_invalid_semantic_versions() {
        for invalid in [
            "0.8",
            "0.8.6.1",
            "0.8.06",
            "0.8.6-",
            "0.8.6-01",
            "0.8.6+",
            "0.8.6+build+again",
        ] {
            assert!(parse_version(invalid).is_err(), "accepted {invalid}");
        }
    }
}
