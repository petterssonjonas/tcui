use std::io::Write;
use std::path::Path;

pub(crate) fn provider_request(provider: &str, endpoint: &str, model: &str) {
    append(
        "provider.request",
        &format!("{provider} {endpoint} model={model}"),
    );
}

pub(crate) fn provider_response(provider: &str, status: reqwest::StatusCode) {
    append("provider.response", &format!("{provider} status={status}"));
}

pub(crate) fn provider_error(provider: &str, message: &str) {
    append("provider.error", &format!("{provider} {message}"));
}

pub(crate) fn provider_migration(provider: &str) {
    append(
        "provider.migration",
        &format!("{provider} legacy endpoint and backend migrated"),
    );
}

fn append(kind: &str, message: &str) {
    let Some(path) = dirs::data_dir() else {
        return;
    };
    append_to_data_dir(&path, kind, message);
}

fn append_to_data_dir(data_dir: &Path, kind: &str, message: &str) {
    let mut path = data_dir.to_path_buf();
    path.push("tcui");
    if std::fs::create_dir_all(&path).is_err() {
        return;
    }
    path.push("tcui.log");

    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };

    let timestamp = chrono::Utc::now().to_rfc3339();
    let safe = crate::llm::auth::redact_secrets(message);
    let _ = writeln!(file, "{timestamp} {kind} {safe}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcui_log_redacts_oauth_token_canaries() {
        let root = std::env::temp_dir().join(format!("tcui-diagnostics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        append_to_data_dir(
            &root,
            "provider.error",
            "Codex access_token=eyJ.tcui-secret-canary refresh_token=tcui-refresh-secret-canary device_code=dc.tcui-device-canary code=AUTHCODE.tcui-canary state=STATE.tcui-canary code_verifier=VERIFIER.tcui-canary authorization code and state remain prose",
        );

        let log = std::fs::read_to_string(root.join("tcui").join("tcui.log"))
            .expect("diagnostics test log should exist");
        assert!(log.contains("provider.error"));
        let leaked = [
            "eyJ.tcui-secret-canary",
            "tcui-refresh-secret-canary",
            "dc.tcui-device-canary",
            "AUTHCODE.tcui-canary",
            "STATE.tcui-canary",
            "VERIFIER.tcui-canary",
        ]
        .into_iter()
        .filter(|canary| log.contains(canary))
        .collect::<Vec<_>>();
        assert!(leaked.is_empty(), "tcui.log leaked {leaked:?}");
        assert!(log.contains("authorization code and state remain prose"));
        let _ = std::fs::remove_dir_all(root);
    }
}
