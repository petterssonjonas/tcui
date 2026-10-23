use super::*;

#[test]
fn openai_stream_events_split_thinking_from_answer() {
    let data = r#"{
        "choices": [{
            "delta": {
                "reasoning_content": "working",
                "content": "answer"
            }
        }]
    }"#;

    let events = openai_stream_events(data).unwrap();

    assert_eq!(
        events.events,
        vec![
            ChatStreamEvent::Thinking("working".to_string()),
            ChatStreamEvent::Answer("answer".to_string())
        ]
    );
}

#[test]
fn stream_events_preserve_literal_tags_across_chunks() {
    let chunks = [
        "<tcui:chat",
        "-title>東京</tcui:chat-title>\n",
        "\"<memory>quoted</memory>\" <tcui:mem",
        "ory>literal</tcui:memory> ",
        "<chat-title>legacy</chat-title> <tcui:chat-title>unfinished ",
        "<memory>unfinished <tcui:mem",
    ];
    let expected: Vec<_> = chunks
        .iter()
        .map(|chunk| ChatStreamEvent::Answer((*chunk).to_owned()))
        .collect();
    let mut openai_events = Vec::new();
    let mut completion_events = Vec::new();
    let mut anthropic_events = Vec::new();
    for text in chunks {
        let openai = serde_json::json!({"choices": [{"delta": {"content": text}}]});
        openai_events.extend(openai_stream_events(&openai.to_string()).unwrap().events);
        let completion = serde_json::json!({"choices": [{"text": text}]});
        completion_events.extend(
            openai_stream_events(&completion.to_string())
                .unwrap()
                .events,
        );
        let anthropic = serde_json::json!({"delta": {"text": text}});
        anthropic_events.extend(anthropic_stream_events(&anthropic.to_string()).unwrap());
    }

    assert_eq!(openai_events, expected);
    assert_eq!(completion_events, expected);
    assert_eq!(anthropic_events, expected);
}

#[test]
fn anthropic_stream_events_split_thinking_from_answer() {
    let events =
        anthropic_stream_events(r#"{"delta":{"thinking":"working","text":"answer"}}"#).unwrap();

    assert_eq!(
        events,
        vec![
            ChatStreamEvent::Thinking("working".to_owned()),
            ChatStreamEvent::Answer("answer".to_owned())
        ]
    );
}

#[test]
fn openai_stream_events_capture_usage_when_present() {
    let data = r#"{"choices":[{"delta":{"content":"answer"}}],"usage":{"total_tokens":2048}}"#;

    let chunk = openai_stream_events(data).unwrap();

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
