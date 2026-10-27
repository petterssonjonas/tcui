use std::sync::{Arc, Barrier};

use chrono::{DateTime, Utc};

use super::test_support::{env_lock, TestEnv};
use super::*;

const CONCURRENT_WRITERS: usize = 16;

#[cfg(unix)]
#[test]
fn concurrent_oauth_upserts_preserve_every_record_and_store_invariant() {
    use std::os::unix::fs::PermissionsExt;

    // Given
    let _guard = env_lock().lock().expect("env lock poisoned");
    let environment = TestEnv::new("oauth-concurrent-upserts");
    let config = environment.config();
    let barrier = Arc::new(Barrier::new(CONCURRENT_WRITERS));

    // When
    let results = std::thread::scope(|scope| {
        let mut workers = Vec::with_capacity(CONCURRENT_WRITERS);
        for index in 0..CONCURRENT_WRITERS {
            let worker_config = config.clone();
            let worker_barrier = Arc::clone(&barrier);
            workers.push(scope.spawn(move || {
                let credential = credential(index);
                worker_barrier.wait();
                KeyStore::upsert_oauth(&worker_config, &credential)
            }));
        }
        workers
            .into_iter()
            .map(|worker| worker.join().expect("OAuth writer should not panic"))
            .collect::<Vec<_>>()
    });

    // Then
    for result in results {
        result.expect("concurrent OAuth upsert should succeed");
    }
    for index in 0..CONCURRENT_WRITERS {
        let expected = credential(index);
        let actual = KeyStore::get_oauth(&config, &expected.provider)
            .expect("concurrent store should remain readable");
        assert_eq!(actual, Some(expected), "OAuth record {index} was lost");
    }
    let path = std::path::Path::new(config.key_file.as_deref().expect("key file path"));
    let raw = std::fs::read_to_string(path).expect("read final key store");
    toml::from_str::<toml::Value>(&raw).expect("final key store should be complete TOML");
    let mode = std::fs::metadata(path)
        .expect("credential file metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let parent = path.parent().expect("key file parent");
    assert!(
        std::fs::read_dir(parent)
            .expect("read key file parent")
            .all(|entry| !entry
                .expect("read directory entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".tcui-keys-")),
        "concurrent persistence left a temporary key-store file"
    );
}

fn credential(index: usize) -> OAuthCredential {
    OAuthCredential {
        provider: format!("Provider-{index:02}"),
        access_token: format!("access-{index:02}"),
        refresh_token: Some(format!("refresh-{index:02}")),
        expires_at: DateTime::parse_from_rfc3339("2030-01-02T03:04:05Z")
            .expect("valid expiry fixture")
            .with_timezone(&Utc),
        account_id: Some(format!("account-{index:02}")),
        ownership: OAuthCredentialOwnership::Tcui,
        source: OAuthCredentialSource::NativeOAuth,
    }
}
