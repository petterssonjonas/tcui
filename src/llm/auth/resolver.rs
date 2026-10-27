use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::config::{AppConfig, KeyStore};

use super::codex::{CodexCredential, CodexNativeAdapter};
use super::credential::Credential;
use super::oauth::oauth_cancellation;
use super::policy::{canonical_provider_name, trusted_provider_endpoint};
use super::{CredentialError, CredentialSource};

#[cfg(test)]
#[path = "resolver_lifecycle_tests.rs"]
mod lifecycle_tests;

pub(crate) struct CredentialRequest<'a> {
    provider: &'a str,
    _env_var: &'a str,
    endpoint: &'a str,
}

impl<'a> CredentialRequest<'a> {
    pub(crate) const fn new(provider: &'a str, env_var: &'a str, endpoint: &'a str) -> Self {
        Self {
            provider,
            _env_var: env_var,
            endpoint,
        }
    }
}

type NativeRefreshResult = Result<CodexCredential, CredentialError>;
type NativeRefreshWaiter = oneshot::Sender<NativeRefreshResult>;

struct NativeRefreshFlight {
    current: Arc<AtomicBool>,
    waiters: Vec<NativeRefreshWaiter>,
    worker: JoinHandle<()>,
    completed: bool,
}

impl NativeRefreshFlight {
    fn abort(self) -> JoinHandle<()> {
        self.current.store(false, Ordering::Release);
        self.worker.abort();
        // Dropping the senders interrupts ordinary waiters without retaining credentials.
        self.worker
    }
}

#[derive(Default)]
struct RootRefreshes {
    flights: HashMap<String, NativeRefreshFlight>,
    retiring: Vec<JoinHandle<()>>,
}

impl RootRefreshes {
    fn reap_finished(&mut self) {
        self.flights
            .retain(|_, flight| !flight.worker.is_finished());
        self.retiring.retain(|worker| !worker.is_finished());
    }
}

fn native_refresh_flights() -> MutexGuard<'static, HashMap<PathBuf, RootRefreshes>> {
    static FLIGHTS: LazyLock<Mutex<HashMap<PathBuf, RootRefreshes>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    FLIGHTS.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn refresh_root() -> PathBuf {
    crate::storage::paths::TcuiDataPaths::discover().database
}

/// Invalidates the exact connection before its credentials are removed or replaced.
/// Aborted handles remain owned until they finish or lifecycle shutdown joins them.
pub(crate) fn invalidate_provider_refreshes(provider: &str) {
    let mut roots = native_refresh_flights();
    if let Some(root) = roots.get_mut(&refresh_root()) {
        root.reap_finished();
        if let Some(flight) = root.flights.remove(provider) {
            root.retiring.push(flight.abort());
        }
    }
}

/// Call after stopping refresh producers and before revoking the current root's keys.
pub(crate) async fn cancel_all_refreshes() {
    let Some(root) = native_refresh_flights().remove(&refresh_root()) else {
        return;
    };
    let mut workers = root.retiring;
    workers.extend(root.flights.into_values().map(NativeRefreshFlight::abort));
    for worker in workers {
        let _ = worker.await;
    }
}

/// Refreshes a rejected native token even when its recorded expiry is still in the future.
pub(crate) async fn refresh_rejected_codex_credential(
    config: &AppConfig,
    connection: &str,
    rejected_access_token: &str,
) -> NativeRefreshResult {
    refresh_native_singleflight(config, None, connection, Some(rejected_access_token)).await
}

pub(crate) async fn resolve_provider_credential(
    request: CredentialRequest<'_>,
) -> Result<Option<Credential>, CredentialError> {
    // Credentials are XDG-database scoped, not config-file scoped. Reloading config.toml
    // here races its exclusive migration/save lock during concurrent provider probes.
    resolve_with_config(request, AppConfig::default(), None).await
}

#[cfg(test)]
pub(crate) async fn resolve_provider_credential_with_native_adapter(
    request: CredentialRequest<'_>,
    config: &AppConfig,
    adapter: &CodexNativeAdapter,
) -> Result<Option<Credential>, CredentialError> {
    resolve_with_config(request, config.clone(), Some(adapter)).await
}

#[cfg(test)]
pub(crate) async fn resolve_provider_credential_with_config(
    request: CredentialRequest<'_>,
    config: &AppConfig,
) -> Result<Option<Credential>, CredentialError> {
    resolve_with_config(request, config.clone(), None).await
}

async fn resolve_with_config(
    request: CredentialRequest<'_>,
    config: AppConfig,
    native_adapter: Option<&CodexNativeAdapter>,
) -> Result<Option<Credential>, CredentialError> {
    if !trusted_provider_endpoint(request.provider, request.endpoint) {
        return Err(CredentialError::UntrustedEndpoint);
    }

    let provider = canonical_provider_name(request.provider);
    let stored_name = if matches!(provider.as_str(), "Codex" | "OpenRouter OAuth") {
        let base = crate::config::key_store::provider_base_name(request.provider);
        std::borrow::Cow::Owned(format!("{provider}{}", &request.provider[base.len()..]))
    } else {
        std::borrow::Cow::Borrowed(request.provider)
    };
    if provider == "Codex" {
        return resolve_codex(&config, native_adapter, &stored_name)
            .await
            .map(|credential| credential.map(Credential::codex));
    }

    // Connections are explicit. Never discover credentials in the environment or other apps.
    if provider == "OpenRouter OAuth" {
        return KeyStore::get_api_key_credential(&config, &stored_name)
            .map_err(|_| CredentialError::CredentialStore)
            .map(|credential| {
                credential.map(|credential| {
                    Credential::api_key(
                        provider,
                        CredentialSource::TcuiStoredApiKey,
                        credential.api_key().to_owned(),
                    )
                })
            });
    }
    let key = KeyStore::get(&config, &stored_name)
        .map_err(|_| CredentialError::CredentialStore)?
        .filter(|value| !value.trim().is_empty());
    Ok(key.map(|key| Credential::api_key(provider, CredentialSource::TcuiStoredApiKey, key)))
}

async fn resolve_codex(
    config: &AppConfig,
    native_adapter: Option<&CodexNativeAdapter>,
    connection: &str,
) -> Result<Option<CodexCredential>, CredentialError> {
    let credential = super::codex::resolve_connection(config, connection)
        .map_err(|_| CredentialError::CredentialStore)?;
    let Some(credential) = credential else {
        return Ok(None);
    };
    if !is_expired(&credential) {
        return Ok(Some(credential));
    }
    drop(credential);

    refresh_native_singleflight(config, native_adapter, connection, None)
        .await
        .map(Some)
}

fn is_expired(credential: &CodexCredential) -> bool {
    credential
        .expires_at()
        .is_some_and(|expiry| expiry <= Utc::now())
}

async fn refresh_native_singleflight(
    config: &AppConfig,
    native_adapter: Option<&CodexNativeAdapter>,
    connection: &str,
    rejected_access_token: Option<&str>,
) -> NativeRefreshResult {
    let (sender, receiver) = oneshot::channel();
    let root_path = refresh_root();
    {
        let mut roots = native_refresh_flights();
        let root = roots.entry(root_path.clone()).or_default();
        root.reap_finished();
        if root
            .flights
            .get(connection)
            .is_some_and(|flight| flight.completed)
        {
            if let Some(flight) = root.flights.remove(connection) {
                root.retiring.push(flight.abort());
            }
        }
        if let Some(flight) = root.flights.get_mut(connection) {
            flight.waiters.push(sender);
        } else {
            let config = config.clone();
            let native_adapter = native_adapter.cloned();
            let connection_name = connection.to_owned();
            let rejected_access_token =
                rejected_access_token.map(|token| SecretString::from(token.to_owned()));
            let current = Arc::new(AtomicBool::new(true));
            let worker_current = Arc::clone(&current);
            let worker = tokio::spawn(async move {
                let result = refresh_after_recheck(
                    &config,
                    native_adapter.as_ref(),
                    &connection_name,
                    rejected_access_token
                        .as_ref()
                        .map(ExposeSecret::expose_secret),
                    &worker_current,
                )
                .await;
                let waiters = {
                    let mut roots = native_refresh_flights();
                    let flight = roots
                        .get_mut(&root_path)
                        .and_then(|root| root.flights.get_mut(&connection_name))
                        .filter(|flight| Arc::ptr_eq(&flight.current, &worker_current));
                    match flight {
                        Some(flight) => {
                            flight.completed = true;
                            std::mem::take(&mut flight.waiters)
                        }
                        None => Vec::new(),
                    }
                };
                for waiter in waiters {
                    let _ = waiter.send(result.clone());
                }
            });
            root.flights.insert(
                connection.to_owned(),
                NativeRefreshFlight {
                    current,
                    waiters: vec![sender],
                    worker,
                    completed: false,
                },
            );
        }
    }
    receiver
        .await
        .map_err(|_| CredentialError::NativeRefreshInterrupted)?
}

async fn refresh_after_recheck(
    config: &AppConfig,
    native_adapter: Option<&CodexNativeAdapter>,
    connection: &str,
    rejected_access_token: Option<&str>,
    current: &AtomicBool,
) -> NativeRefreshResult {
    let credential = super::codex::resolve_connection(config, connection)
        .map_err(|_| CredentialError::CredentialStore)?
        .ok_or(CredentialError::CodexCredentialUnavailable)?;
    let rejected = rejected_access_token
        .is_some_and(|token| credential.access_token().expose_secret() == token);
    if !is_expired(&credential) && !rejected {
        return Ok(credential);
    }
    drop(credential);

    let production_adapter;
    let adapter = match native_adapter {
        Some(adapter) => adapter,
        None => {
            production_adapter = CodexNativeAdapter::production()
                .map_err(|_| CredentialError::NativeRefreshFailed)?;
            &production_adapter
        }
    };
    let (cancellation, _handle) = oauth_cancellation();
    adapter
        .clone()
        .for_connection(connection)
        .refresh(config, &cancellation, || current.load(Ordering::Acquire))
        .await
        .map_err(|_| CredentialError::NativeRefreshFailed)
}
