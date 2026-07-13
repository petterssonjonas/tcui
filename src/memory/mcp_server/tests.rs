use std::fs;
use std::path::Path;

use rmcp::{ServiceExt, model::CallToolRequestParams};

use super::{MemoryMcp, MemoryStore};
use crate::memory::WriteOutcome;

#[tokio::test]
async fn stdio_protocol_lists_and_calls_memory_tools() {
    // Given
    let _guard = crate::test_support::env_lock()
        .lock()
        .expect("env lock poisoned");
    let vault = std::env::temp_dir().join(format!("tcui-memory-mcp-{}", rand::random::<u64>()));
    let data_home = vault.join("data-home");
    fs::create_dir_all(&data_home).expect("data home");
    std::env::set_var("XDG_DATA_HOME", &data_home);
    fs::create_dir_all(vault.join("memories")).expect("temporary vault");
    let store = MemoryStore::open(&vault, &vault.join("memory.sqlite3")).expect("memory store");
    let WriteOutcome::Saved { .. } = store
        .write(
            Path::new("fact.md"),
            "# Fact\n\nRust is preferred.\n",
            false,
        )
        .expect("memory note")
    else {
        panic!("memory should be saved");
    };
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server = tokio::spawn(async move {
        MemoryMcp::new(store)
            .serve(server_transport)
            .await?
            .waiting()
            .await?;
        color_eyre::Result::<()>::Ok(())
    });
    let client = ().serve(client_transport).await.expect("MCP client");

    // When
    let tools = client.list_all_tools().await.expect("list tools");
    let search_arguments = serde_json::json!({"query": "Rust editor", "limit": 3})
        .as_object()
        .cloned()
        .expect("search arguments");
    let (status, search, resources) = tokio::join!(
        client.call_tool(CallToolRequestParams::new("memory_status")),
        client.call_tool(
            CallToolRequestParams::new("memory_search").with_arguments(search_arguments)
        ),
        client.list_all_resources(),
    );
    let status = status.expect("call status");
    let search = search.expect("call search");
    let resources = resources.expect("list resources");

    // Then
    assert_eq!(tools.len(), 6);
    assert_ne!(status.is_error, Some(true));
    assert_ne!(search.is_error, Some(true));
    assert_eq!(resources.len(), 2);
    client.cancel().await.expect("cancel client");
    server.await.expect("server task").expect("memory server");
    fs::remove_dir_all(vault).expect("temporary vault cleanup");
    std::env::remove_var("XDG_DATA_HOME");
}
