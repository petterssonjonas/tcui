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
