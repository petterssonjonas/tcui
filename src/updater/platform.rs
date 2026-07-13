use std::path::PathBuf;

use color_eyre::{Result, eyre::eyre};

pub(super) fn asset_name_for_platform() -> Result<String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("tcui-x86_64-unknown-linux-gnu.tar.gz".to_string()),
        (os, arch) => Err(eyre!("Upgrade is not supported on {os}/{arch} yet")),
    }
}

pub(super) fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
}
