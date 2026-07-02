use crate::config::AppConfig;

const FINISHED_TITLE: &str = "TermChatUI response ready";
const REMINDER_TITLE: &str = "TermChatUI reminder";
const MAX_BODY_CHARS: usize = 280;

pub(crate) async fn notify_finished(
    config: &AppConfig,
    prompt: &str,
    answer: &str,
) -> color_eyre::Result<()> {
    let body = notification_body(prompt, answer);
    if body.is_empty() {
        return Ok(());
    }

    notify_text(config, FINISHED_TITLE, &body, false).await
}

pub(crate) async fn notify_reminder(config: &AppConfig, message: &str) -> color_eyre::Result<()> {
    let body = truncate_chars(message.trim(), MAX_BODY_CHARS);
    if body.is_empty() {
        return Ok(());
    }
    notify_text(config, REMINDER_TITLE, &body, true).await
}

fn notification_body(prompt: &str, answer: &str) -> String {
    let summary = first_non_empty_line(answer).unwrap_or(prompt).trim();
    truncate_chars(summary, MAX_BODY_CHARS)
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let truncated: String = text.chars().take(max_chars).collect();
    if truncated.len() == text.len() {
        truncated
    } else {
        format!("{truncated}...")
    }
}

fn ntfy_url(config: &AppConfig) -> Option<String> {
    let ntfy = &config.notifications.ntfy;
    if !ntfy.enabled {
        return None;
    }
    let host = ntfy.host.trim();
    if host.is_empty() {
        return None;
    }
    let scheme_host = if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else {
        format!("http://{host}")
    };
    let base = scheme_host.trim_end_matches('/');
    let topic = ntfy.topic.trim().trim_matches('/');
    let topic = if topic.is_empty() { "tcui" } else { topic };
    Some(format!("{base}:{ntfy_port}/{topic}", ntfy_port = ntfy.port))
}

async fn notify_text(
    config: &AppConfig,
    title: &str,
    body: &str,
    audible_bell: bool,
) -> color_eyre::Result<()> {
    if config.notifications.desktop {
        let _ = send_desktop(title, body).await;
    }
    if let Some(url) = ntfy_url(config) {
        let _ = send_ntfy(&url, title, body).await;
    }
    if audible_bell {
        let _ = send_bell().await;
    }
    Ok(())
}

async fn send_desktop(title: &str, body: &str) -> std::io::Result<()> {
    let _status = tokio::process::Command::new("notify-send")
        .arg(title)
        .arg(body)
        .status()
        .await?;
    Ok(())
}

async fn send_ntfy(url: &str, title: &str, body: &str) -> reqwest::Result<()> {
    reqwest::Client::new()
        .post(url)
        .header("Title", title)
        .body(body.to_string())
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn send_bell() -> std::io::Result<()> {
    for (program, args) in [
        ("canberra-gtk-play", vec!["-i", "bell"]),
        (
            "paplay",
            vec!["/usr/share/sounds/freedesktop/stereo/bell.oga"],
        ),
    ] {
        let status = tokio::process::Command::new(program)
            .args(&args)
            .status()
            .await;
        if matches!(status, Ok(exit) if exit.success()) {
            return Ok(());
        }
    }
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(b"\x07");
    let _ = stdout.flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{notification_body, ntfy_url};

    #[test]
    fn ntfy_url_uses_default_scheme_and_topic() {
        let mut config = crate::config::AppConfig::default();
        config.notifications.ntfy.enabled = true;
        config.notifications.ntfy.host = "127.0.0.1".to_string();
        config.notifications.ntfy.port = 8080;
        config.notifications.ntfy.topic.clear();

        assert_eq!(
            ntfy_url(&config).as_deref(),
            Some("http://127.0.0.1:8080/tcui")
        );
    }

    #[test]
    fn notification_body_prefers_answer_excerpt() {
        let body = notification_body("original prompt", "\n\nFinished answer\nextra");

        assert_eq!(body, "Finished answer");
    }
}
