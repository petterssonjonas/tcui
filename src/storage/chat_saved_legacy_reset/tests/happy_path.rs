use crate::config::vault_profiles::VaultProfile;

use super::super::LegacyResetOutcome;
use super::support::TestRoot;

#[test]
fn chat_saved_legacy_reset_removes_only_eligible_legacy_entries() {
    // Given
    let root = TestRoot::new("eligible-entries");
    let saved_root = root.create_dir("saved");
    root.write("saved/legacy.md", b"legacy saved");
    root.write("saved/nested/kept.md", b"nested saved sentinel");
    root.write(
        "cache/tcui/chat-files/42/payload.bin",
        b"legacy chat payload",
    );
    let outside_target = root.write("outside/target.md", b"outside symlink target");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside_target, root.path("saved/legacy-link.md"))
        .expect("create Saved symlink");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        root.create_dir("outside/cache-target"),
        root.path("cache/tcui/chat-files/cache-link"),
    )
    .expect("create cache symlink");
    root.write("outside/cache-target/sentinel", b"cache symlink target");

    let profile_a = root.create_dir("vaults/profile-a");
    let profile_b = root.create_dir("vaults/profile-b");
    let legacy_vault = root.create_dir("vaults/legacy");
    let active_vault = root.create_dir("vaults/active");
    root.write("vaults/profile-a/a.md", b"profile a sentinel");
    root.write("vaults/profile-b/b.md", b"profile b sentinel");
    root.write("vaults/legacy/legacy.md", b"legacy vault sentinel");
    root.write("vaults/active/active.md", b"active vault sentinel");
    root.write("data/tcui/vault/fallback.md", b"fallback vault sentinel");
    root.write("data/tcui/memories/memory.bin", b"memory sentinel");
    root.write("data/tcui/tcui.db", b"database sentinel");
    root.write("data/tcui/chat.key", b"chat key sentinel");
    root.write("data/tcui/keys.toml", b"provider key sentinel");
    root.write("config/tcui/config.toml", b"provider config sentinel");

    let mut config = root.config(&saved_root);
    let first = VaultProfile::new("Profile A", &profile_a).expect("create first profile");
    let second = VaultProfile::new("Profile B", &profile_b).expect("create second profile");
    config.vaults.insert(first.id.clone(), first);
    config.vaults.insert(second.id.clone(), second);
    config.vault_path = Some(legacy_vault.display().to_string());
    let reset = root.reset(&config, Some(&active_vault));

    // When
    let outcome = reset.run().expect("run legacy reset");

    // Then
    assert_eq!(outcome, LegacyResetOutcome::Completed);
    assert!(!root.path("cache/tcui/chat-files").exists());
    assert!(!root.path("saved/legacy.md").exists());
    #[cfg(unix)]
    assert!(!root.path("saved/legacy-link.md").exists());
    assert_eq!(root.read("saved/nested/kept.md"), b"nested saved sentinel");
    assert_eq!(
        std::fs::read(outside_target).expect("read target"),
        b"outside symlink target"
    );
    assert_eq!(
        root.read("outside/cache-target/sentinel"),
        b"cache symlink target"
    );
    assert_eq!(root.read("vaults/profile-a/a.md"), b"profile a sentinel");
    assert_eq!(root.read("vaults/profile-b/b.md"), b"profile b sentinel");
    assert_eq!(
        root.read("vaults/legacy/legacy.md"),
        b"legacy vault sentinel"
    );
    assert_eq!(
        root.read("vaults/active/active.md"),
        b"active vault sentinel"
    );
    assert_eq!(
        root.read("data/tcui/vault/fallback.md"),
        b"fallback vault sentinel"
    );
    assert_eq!(
        root.read("data/tcui/memories/memory.bin"),
        b"memory sentinel"
    );
    assert_eq!(root.read("data/tcui/tcui.db"), b"database sentinel");
    assert_eq!(root.read("data/tcui/chat.key"), b"chat key sentinel");
    assert_eq!(root.read("data/tcui/keys.toml"), b"provider key sentinel");
    assert_eq!(
        root.read("config/tcui/config.toml"),
        b"provider config sentinel"
    );
    assert!(root.marker_path().is_file());
}
