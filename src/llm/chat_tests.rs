use super::*;

#[test]
fn title_tag_filter_strips_split_control_tag() {
    let mut filter = TitleTagFilter::default();

    let first = filter.push("<tcui:chat");
    let second = filter.push("-title>Hidden");
    let third = filter.push("</tcui:chat-title>\nok");
    let tail = filter.finish();

    assert_eq!(first, TitleFilteredChunk::default());
    assert_eq!(second, TitleFilteredChunk::default());
    assert_eq!(third.visible, "\nok");
    assert_eq!(third.title, Some("Hidden".to_string()));
    assert_eq!(tail, TitleFilteredChunk::default());
}

#[test]
fn openai_stream_events_split_thinking_from_answer() {
    let mut filter = TitleTagFilter::default();
    let data = r#"{
        "choices": [{
            "delta": {
                "reasoning_content": "working",
                "content": "answer"
            }
        }]
    }"#;

    let events = openai_stream_events(data, &mut filter).unwrap();

    assert_eq!(
        events.events,
        vec![
            ChatStreamEvent::Thinking("working".to_string()),
            ChatStreamEvent::Answer("answer".to_string())
        ]
    );
}

#[test]
fn openai_stream_events_strips_title_from_answer() {
    let mut filter = TitleTagFilter::default();
    let first = r#"{"choices":[{"delta":{"content":"<tcui:chat-title>Hidden"}}]}"#;
    let second = r#"{"choices":[{"delta":{"content":"</tcui:chat-title>\nok"}}]}"#;

    let mut events = openai_stream_events(first, &mut filter).unwrap().events;
    events.extend(openai_stream_events(second, &mut filter).unwrap().events);

    assert_eq!(
        events,
        vec![
            ChatStreamEvent::Title("Hidden".to_string()),
            ChatStreamEvent::Answer("\nok".to_string())
        ]
    );
}

#[test]
fn legacy_chat_title_tag_remains_visible_text() {
    let mut filter = TitleTagFilter::default();

    let first = filter.push("<chat-title>Old");
    let second = filter.push("</chat-title>");
    let tail = filter.finish();

    assert_eq!(first.visible, "<chat-title>Old");
    assert_eq!(first.title, None);
    assert_eq!(second.visible, "</chat-title>");
    assert_eq!(second.title, None);
    assert_eq!(tail, TitleFilteredChunk::default());
}

#[test]
fn openai_stream_events_capture_usage_when_present() {
    let mut filter = TitleTagFilter::default();
    let data = r#"{"choices":[{"delta":{"content":"answer"}}],"usage":{"total_tokens":2048}}"#;

    let chunk = openai_stream_events(data, &mut filter).unwrap();

    assert_eq!(chunk.total_tokens, Some(2048));
    assert_eq!(
        chunk.events,
        vec![ChatStreamEvent::Answer("answer".to_string())]
    );
}

#[test]
fn consume_sse_bytes_waits_for_split_utf8() {
    let mut buffer = Vec::new();
    let mut seen = Vec::new();
    let full = "data: {\"choices\":[{\"delta\":{\"content\":\"å\"}}]}\n\n";
    let split_at = full.find('å').unwrap() + 1;

    buffer.extend_from_slice(&full.as_bytes()[..split_at]);
    assert!(consume_sse_bytes(&mut buffer, &mut |data| {
        seen.push(data.to_string());
        Ok(true)
    })
    .unwrap());
    assert!(seen.is_empty());

    buffer.extend_from_slice(&full.as_bytes()[split_at..]);
    assert!(consume_sse_bytes(&mut buffer, &mut |data| {
        seen.push(data.to_string());
        Ok(true)
    })
    .unwrap());

    assert_eq!(
        seen,
        vec!["{\"choices\":[{\"delta\":{\"content\":\"å\"}}]}".to_string()]
    );
    assert!(buffer.is_empty());
}

#[test]
fn consume_sse_bytes_stops_on_done() {
    let mut buffer = b"data: {\"ok\":true}\n\ndata: [DONE]\n\ndata: {\"late\":true}\n\n".to_vec();
    let mut seen = Vec::new();

    let keep_going = consume_sse_bytes(&mut buffer, &mut |data| {
        seen.push(data.to_string());
        Ok(true)
    })
    .unwrap();

    assert!(!keep_going);
    assert_eq!(seen, vec!["{\"ok\":true}".to_string()]);
}
