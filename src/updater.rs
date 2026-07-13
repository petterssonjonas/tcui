use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::{Result, eyre::eyre};
use serde::Deserialize;

mod platform;
mod version;

use platform::{asset_name_for_platform, unique_temp_dir};
use version::{normalize_version, parse_version};

pub const RELEASE_REPO: &str = "petterssonjonas/tcui";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const USER_AGENT: &str = concat!("tcui/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub asset_name: String,
    pub asset_url: String,
    pub sums_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn available_release() -> Result<Option<ReleaseInfo>> {
    let client = github_client()?;
    let Some(release) = latest_release(&client).await? else {
        return Ok(None);
    };
    if parse_version(&release.version)? <= parse_version(CURRENT_VERSION)? {
        return Ok(None);
    }
    Ok(Some(release))
}

pub async fn upgrade_to_latest() -> Result<String> {
    let client = github_client()?;
    let Some(release) = latest_release(&client).await? else {
        return Ok(format!(
            "No published GitHub release is available yet for {}.",
            RELEASE_REPO
        ));
    };

    let latest = parse_version(&release.version)?;
    let current = parse_version(CURRENT_VERSION)?;
    if latest <= current {
        return Ok(format!("tcui {CURRENT_VERSION} is already up to date."));
    }

    let install_path = install_target_path()?;
    let temp_dir = unique_temp_dir("upgrade");
    std::fs::create_dir_all(&temp_dir)?;
    let archive_path = temp_dir.join(&release.asset_name);
    let sums_path = temp_dir.join("SHA256SUMS");

    download_to_path(&client, &release.asset_url, &archive_path).await?;
    download_to_path(&client, &release.sums_url, &sums_path).await?;
    verify_archive_checksum(&archive_path, &sums_path, &release.asset_name)?;
    unpack_archive(&archive_path, &temp_dir)?;
    install_binary(&temp_dir.join("tcui"), &install_path)?;

    Ok(format!(
        "Updated tcui to {} at {}",
        release.version,
        install_path.display()
    ))
}

fn github_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(USER_AGENT)
        .build()?)
}

async fn latest_release(client: &reqwest::Client) -> Result<Option<ReleaseInfo>> {
    let response = client
        .get(format!(
            "https://api.github.com/repos/{}/releases/latest",
            RELEASE_REPO
        ))
        .send()
        .await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let response = response.error_for_status()?;
    let release = response.json::<GithubRelease>().await?;
    let version = normalize_version(&release.tag_name);
    let asset_name = asset_name_for_platform()?;
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| eyre!("Release {} does not contain asset {}", version, asset_name))?;

    let tag = release.tag_name;
    Ok(Some(ReleaseInfo {
        tag: tag.clone(),
        version,
        asset_name: asset.name,
        asset_url: asset.browser_download_url,
        sums_url: format!(
            "https://github.com/{}/releases/download/{}/SHA256SUMS",
            RELEASE_REPO, tag
        ),
    }))
}

async fn download_to_path(client: &reqwest::Client, url: &str, path: &Path) -> Result<()> {
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    std::fs::write(path, &bytes)?;
    Ok(())
}

fn install_target_path() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("TCUI_BIN_DIR") {
        return Ok(PathBuf::from(dir).join("tcui"));
    }

    let current = std::env::current_exe()?;
    if should_replace_current_binary(&current) {
        return Ok(current);
    }

    let Some(home) = dirs::home_dir() else {
        return Err(eyre!("Could not resolve home directory for upgrade target"));
    };
    Ok(home.join(".local/bin/tcui"))
}

fn should_replace_current_binary(current: &Path) -> bool {
    if current.file_name().and_then(|name| name.to_str()) != Some("tcui") {
        return false;
    }
    if looks_like_dev_binary(current) {
        return false;
    }
    current.parent().is_some_and(directory_is_writable)
}

fn looks_like_dev_binary(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "target")
    })
}

fn directory_is_writable(path: &Path) -> bool {
    let probe = path.join(format!(
        ".tcui-write-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe);
    match result {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

fn verify_archive_checksum(archive_path: &Path, sums_path: &Path, asset_name: &str) -> Result<()> {
    let sums = std::fs::read_to_string(sums_path)?;
    let selected = sums
        .lines()
        .find(|line| line.ends_with(asset_name))
        .ok_or_else(|| eyre!("SHA256SUMS does not contain {}", asset_name))?;
    let selected_path = sums_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("SHA256SUMS.selected");
    std::fs::write(&selected_path, format!("{selected}\n"))?;

    let verify = |program: &str, args: &[&str]| -> Result<bool> {
        let status = std::process::Command::new(program)
            .args(args)
            .current_dir(
                archive_path
                    .parent()
                    .ok_or_else(|| eyre!("Archive path has no parent"))?,
            )
            .status();
        match status {
            Ok(status) => Ok(status.success()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    };

    if verify("sha256sum", &["-c", "SHA256SUMS.selected"])? {
        return Ok(());
    }
    if verify("shasum", &["-a", "256", "-c", "SHA256SUMS.selected"])? {
        return Ok(());
    }
    Err(eyre!(
        "Could not verify release checksum. Install sha256sum or shasum."
    ))
}

fn unpack_archive(archive_path: &Path, temp_dir: &Path) -> Result<()> {
    let status = std::process::Command::new("tar")
        .args(["-xzf"])
        .arg(archive_path)
        .args(["-C"])
        .arg(temp_dir)
        .status()?;
    if status.success() {
        return Ok(());
    }
    Err(eyre!("Failed to unpack release archive"))
}

fn install_binary(source: &Path, target: &Path) -> Result<()> {
    let Some(parent) = target.parent() else {
        return Err(eyre!("Invalid target path {}", target.display()));
    };
    std::fs::create_dir_all(parent)?;
    let staged = parent.join(format!(
        ".tcui-upgrade-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::copy(source, &staged)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&staged)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&staged, permissions)?;
    }
    std::fs::rename(&staged, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::should_replace_current_binary;

    #[test]
    fn dev_target_binaries_are_not_replaced_in_place() {
        let path = PathBuf::from("/tmp/project/target/debug/tcui");
        assert!(!should_replace_current_binary(&path));
    }
}
