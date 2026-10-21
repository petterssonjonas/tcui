use super::test_support::{env_lock, TestEnv};
use super::*;

#[test]
fn concurrent_oauth_upserts_preserve_every_record() {
    let _guard = env_lock().lock().expect("env lock");
    let env = TestEnv::new("vault-concurrent");
    let config = env.initialized_config();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
    let workers: Vec<_> = (0..16)
        .map(|index| {
            let config = config.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let credential = OAuthCredential {
                    provider: format!("Provider-{index}"),
                    access_token: format!("access-{index}"),
                    refresh_token: None,
                    expires_at: chrono::Utc::now(),
                    account_id: None,
                    ownership: OAuthCredentialOwnership::Tcui,
                    source: OAuthCredentialSource::NativeOAuth,
                };
                barrier.wait();
                KeyStore::upsert_oauth(&config, &credential).expect("write");
                credential
            })
        })
        .collect();
    for worker in workers {
        let expected = worker.join().expect("worker");
        assert_eq!(
            KeyStore::get_oauth(&config, &expected.provider).expect("read"),
            Some(expected)
        );
    }
}
