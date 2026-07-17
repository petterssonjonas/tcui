use std::path::Path;

use super::{SkillCatalog, SkillOrigin};

#[test]
fn vault_target_resolution_builtin_aliases_share_canonical_source() {
    let catalog = SkillCatalog::discover_from(Path::new("skills"), None).expect("skill catalog");

    let vault = catalog.load("vault").expect("load vault").expect("vault");
    let obsidian = catalog
        .load("obsidian")
        .expect("load obsidian")
        .expect("obsidian alias");

    assert_eq!(vault.origin, SkillOrigin::Builtin);
    assert_eq!(obsidian.origin, SkillOrigin::Builtin);
    assert_eq!(vault.source, obsidian.source);
}
