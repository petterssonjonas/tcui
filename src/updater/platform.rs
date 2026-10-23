use std::path::PathBuf;

use color_eyre::{eyre::eyre, Result};

pub(super) fn asset_name_for_platform() -> Result<String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") if cfg!(target_env = "gnu") => {
            Ok("tcui-x86_64-unknown-linux-gnu.tar.gz".to_string())
        }
        (os, arch) => Err(eyre!(
            "Prebuilt upgrades require GNU/Linux x86-64; rebuild from source on {os}/{arch}"
        )),
    }
}

pub(super) fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("tcui-{label}-{}-{nanos}", std::process::id()))
}
