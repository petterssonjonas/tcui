use std::io::Write;

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

fn append(kind: &str, message: &str) {
    let Some(mut path) = dirs::data_dir() else {
        return;
    };
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
