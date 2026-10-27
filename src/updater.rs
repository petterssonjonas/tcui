use std::time::Duration;

use color_eyre::{eyre::eyre, Result};
use serde::Deserialize;

mod install;
mod platform;
mod version;

use platform::asset_name_for_platform;
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
    draft: bool,
    published_at: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn available_release() -> Result<Option<ReleaseInfo>> {
    let client = github_client()?;
    let release = latest_release(&client).await?;
    newer_release(release, CURRENT_VERSION)
}

pub async fn upgrade_to_latest() -> Result<String> {
    let client = github_client()?;
    let Some(release) = latest_release(&client).await? else {
        return Ok(format!(
            "No published GitHub release is available yet for {}.",
            RELEASE_REPO
        ));
    };
    let Some(release) = newer_release(Some(release), CURRENT_VERSION)? else {
        return Ok(format!("tcui {CURRENT_VERSION} is already up to date."));
    };
    install::install_release(&release).await
}

fn github_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .build()?)
}

async fn latest_release(client: &reqwest::Client) -> Result<Option<GithubRelease>> {
    // /latest excludes prereleases. Read all published releases so API ordering
    // and an old assetless stable release cannot hide a newer version.
    let mut newest = None;
    for page in 1_u64.. {
        let response = client
            .get(format!(
                "https://api.github.com/repos/{RELEASE_REPO}/releases?per_page=100&page={page}"
            ))
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(newest);
        }
        let releases = response
            .error_for_status()?
            .json::<Vec<GithubRelease>>()
            .await?;
        let last_page = releases.len() < 100;
        newest = select_newest(newest.into_iter().chain(releases));
        if last_page {
            break;
        }
    }
    Ok(newest)
}

fn select_newest(releases: impl IntoIterator<Item = GithubRelease>) -> Option<GithubRelease> {
    let mut newest: Option<GithubRelease> = None;
    for release in releases {
        if release.draft || release.published_at.is_none() {
            continue;
        }
        let replace = match &newest {
            None => true,
            Some(selected) => release.published_at > selected.published_at,
        };
        if replace {
            newest = Some(release);
        }
    }
    newest
}

fn newer_release(release: Option<GithubRelease>, current: &str) -> Result<Option<ReleaseInfo>> {
    let Some(release) = release else {
        return Ok(None);
    };
    // A current or newer installation needs no assets, and must not downgrade.
    if parse_version(&release.tag_name)? <= parse_version(current)? {
        return Ok(None);
    }
    let asset_name = asset_name_for_platform()?;
    let find_asset = |name: &str| -> Result<String> {
        let mut matches = release.assets.iter().filter(|asset| asset.name == name);
        let asset = matches.next().ok_or_else(|| {
            eyre!(
                "Newest published release {} is incomplete: missing {}. No older release was selected; retry after its assets are published.",
                release.tag_name,
                name
            )
        })?;
        if matches.next().is_some() {
            return Err(eyre!(
                "Release {} has duplicate asset {}",
                release.tag_name,
                name
            ));
        }
        Ok(asset.browser_download_url.clone())
    };
    let asset_url = find_asset(&asset_name)?;
    let sums_url = find_asset("SHA256SUMS")?;
    Ok(Some(ReleaseInfo {
        version: normalize_version(&release.tag_name),
        tag: release.tag_name,
        asset_name,
        asset_url,
        sums_url,
    }))
}

#[cfg(test)]
mod tests;
