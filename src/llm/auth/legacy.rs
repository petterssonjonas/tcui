use crate::config::{AppConfig, KeyStore};
use crate::storage::Storage;

use super::policy::{canonical_provider_name, is_oauth_provider, trusted_provider_endpoint};
use super::reader::{read_env_file, read_oauth_token};

pub(crate) fn read_provider_api_key(
    name: &str,
    env_var: &str,
    storage: &Storage,
) -> Option<String> {
    let endpoint = storage.get_provider_endpoint(name).ok().flatten()?;
    if !trusted_provider_endpoint(name, &endpoint) {
        return None;
    }

    std::env::var(env_var)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| read_env_file(env_var))
        .or_else(|| {
            if is_oauth_provider(name) {
                None
            } else {
                let config = AppConfig::load().ok()?;
                KeyStore::get(&config, &canonical_provider_name(name))
                    .ok()
                    .flatten()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| {
                        KeyStore::get_api_key_credential(&config, &canonical_provider_name(name))
                            .ok()
                            .flatten()
                            .map(|credential| credential.api_key().to_owned())
                    })
            }
        })
        .or_else(|| read_oauth_token(name))
}
