use serde::{Deserialize, Serialize};

use crate::app::Message;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum MemoryActivity {
    Recalling,
    Recalled { titles: Vec<String> },
    Saving,
    Saved { title: String, path: String },
    AlreadyKnown { title: String },
    Failed { message: String },
}

pub(crate) fn activities(message: &Message) -> Vec<MemoryActivity> {
    if message.tool_source.as_deref() != Some("memory") {
        return Vec::new();
    }
    message
        .tool_result
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

pub(crate) fn set_activities(
    message: &mut Message,
    activities: &[MemoryActivity],
) -> serde_json::Result<()> {
    if activities.is_empty() {
        if message.tool_source.as_deref() == Some("memory") {
            message.tool_source = None;
            message.tool_result = None;
        }
        return Ok(());
    }
    message.tool_source = Some("memory".to_string());
    message.tool_result = Some(serde_json::to_string(activities)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MemoryActivity;

    #[test]
    fn activity_round_trips_through_persisted_json() {
        // Given
        let activities = vec![
            MemoryActivity::Recalled {
                titles: vec!["Preferred editor".to_string()],
            },
            MemoryActivity::Saved {
                title: "Rust preference".to_string(),
                path: "preferences/rust.md".to_string(),
            },
        ];

        // When
        let json = serde_json::to_string(&activities).expect("serialize memory activity");
        let decoded: Vec<MemoryActivity> =
            serde_json::from_str(&json).expect("deserialize memory activity");

        // Then
        assert_eq!(decoded, activities);
    }

    #[test]
    fn clearing_activity_removes_memory_tool_metadata() {
        // Given
        let mut message = crate::app::Message::new(1, "assistant".to_string(), String::new());
        super::set_activities(&mut message, &[MemoryActivity::Recalling])
            .expect("set memory activity");

        // When
        super::set_activities(&mut message, &[]).expect("clear memory activity");

        // Then
        assert_eq!(message.tool_source, None);
        assert_eq!(message.tool_result, None);
    }
}
