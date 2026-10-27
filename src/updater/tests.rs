use super::{newer_release, select_newest, GithubRelease};

const ASSET: &str = "tcui-x86_64-unknown-linux-gnu.tar.gz";

fn release(tag: &str, published: &str, prerelease: bool, assets: &[&str]) -> GithubRelease {
    serde_json::from_value(serde_json::json!({
        "tag_name": tag,
        "draft": false,
        "prerelease": prerelease,
        "published_at": published,
        "assets": assets.iter().map(|name| serde_json::json!({
            "name": name,
            "browser_download_url": format!("https://github.com/example/tcui/releases/download/{tag}/{name}")
        })).collect::<Vec<_>>()
    })).unwrap()
}

#[test]
fn published_prerelease_wins_over_old_assetless_stable_regardless_of_api_order() {
    let old = release("v0.8.5", "2026-10-20T12:00:00Z", false, &[]);
    let new = release(
        "0.8.6",
        "2026-10-27T12:00:00Z",
        true,
        &[ASSET, "SHA256SUMS"],
    );
    let selected = select_newest([new, old]);
    let update = newer_release(selected, "0.8.5").unwrap().unwrap();
    assert_eq!(update.tag, "0.8.6");
    assert_eq!(update.version, "0.8.6");
    assert!(update.asset_url.contains("/download/0.8.6/"));
}

#[test]
fn release_tag_keeps_its_optional_v_prefix_for_installation() {
    for tag in ["v0.8.6", "0.8.6"] {
        let selected = release(tag, "2026-10-27T12:00:00Z", true, &[ASSET, "SHA256SUMS"]);
        let update = newer_release(Some(selected), "v0.8.5").unwrap().unwrap();
        assert_eq!(update.tag, tag);
        assert_eq!(update.version, "0.8.6");
    }
}

#[test]
fn drafts_and_unpublished_releases_are_not_selected() {
    let published = release("0.8.6", "2026-10-27T12:00:00Z", true, &[]);
    let mut draft = release("9.0.0", "2026-10-28T12:00:00Z", false, &[]);
    draft.draft = true;
    let mut unpublished = release("10.0.0", "2026-10-29T12:00:00Z", false, &[]);
    unpublished.published_at = None;
    assert_eq!(
        select_newest([draft, published, unpublished])
            .unwrap()
            .tag_name,
        "0.8.6"
    );
}

#[test]
fn publication_time_not_version_or_creation_order_selects_release_without_downgrading() {
    let older_publication = release("1.0.0", "2026-10-26T12:00:00Z", false, &[]);
    let newest_publication = release("0.8.6", "2026-10-27T12:00:00Z", true, &[]);
    let selected = select_newest([newest_publication, older_publication]);
    assert!(newer_release(selected, "1.0.0").unwrap().is_none());
}

#[test]
fn current_or_newer_install_needs_no_release_assets() {
    for current in ["0.8.6", "0.8.7", "1.0.0"] {
        let selected = release("v0.8.6", "2026-10-27T12:00:00Z", false, &[]);
        assert!(newer_release(Some(selected), current).unwrap().is_none());
    }
}

#[test]
fn missing_newest_assets_fail_without_falling_back_to_complete_old_release() {
    for assets in [vec![], vec![ASSET], vec!["SHA256SUMS"]] {
        let old = release(
            "v0.8.5",
            "2026-10-20T12:00:00Z",
            false,
            &[ASSET, "SHA256SUMS"],
        );
        let new = release("0.8.6", "2026-10-27T12:00:00Z", true, &assets);
        let selected = select_newest([old, new]);
        assert!(newer_release(selected, "0.8.4").is_err());
    }
}

#[test]
fn stable_release_updates_an_installed_prerelease_with_the_same_core_version() {
    let selected = release(
        "v0.8.6",
        "2026-10-27T12:00:00Z",
        false,
        &[ASSET, "SHA256SUMS"],
    );
    assert!(newer_release(Some(selected), "0.8.6-rc.1")
        .unwrap()
        .is_some());
}
