use std::fs;
use std::path::Path;

use rmcp::{model::CallToolRequestParams, ServiceExt};

use super::{MemoryMcp, MemoryStore};
use crate::memory::WriteOutcome;

#[test]
fn stdio_protocol_lists_and_calls_memory_tools() {
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
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
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
        });
    fs::remove_dir_all(vault).expect("temporary vault cleanup");
    std::env::remove_var("XDG_DATA_HOME");
}

#[test]
fn memory_mcp_rejects_content_and_mutation_after_session_lock() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let fixture = crate::memory::protection_tests::Fixture::new();
    fixture.configure(true);
    let store = MemoryStore::open_default().expect("protected store");
    store
        .write(
            Path::new("fact.md"),
            "# Fact\n\nRust is preferred.\n",
            false,
        )
        .expect("protected note");
    let server = MemoryMcp::new(store);
    crate::protection::lock().expect("lock");
    assert!(server
        .memory_read(rmcp::handler::server::wrapper::Parameters(
            super::ReadParams {
                path: "fact.md".to_string()
            },
        ))
        .is_err());
    assert!(server
        .memory_search(rmcp::handler::server::wrapper::Parameters(
            super::SearchParams {
                query: "Rust".to_string(),
                limit: 1
            },
        ))
        .is_err());
    assert!(server.memory_status().is_err());
    assert!(server.memory_reindex().is_err());
    assert!(server
        .memory_write(rmcp::handler::server::wrapper::Parameters(
            super::WriteParams {
                path: "blocked.md".to_string(),
                markdown: "Blocked".to_string(),
                overwrite: false
            },
        ))
        .is_err());
    assert!(server
        .memory_forget(rmcp::handler::server::wrapper::Parameters(
            super::ForgetParams {
                path: "fact.md".to_string()
            },
        ))
        .is_err());
}

#[test]
fn memory_mcp_respects_app_entry_lock_even_when_memory_is_unprotected() {
    let _guard = crate::test_support::env_lock().lock().expect("env lock");
    let _fixture = crate::memory::protection_tests::Fixture::new();
    crate::protection::configure(
        crate::protection::ProtectionPolicy {
            app_password: true,
            ..crate::protection::ProtectionPolicy::default()
        },
        "memory-test-password",
    )
    .expect("app entry protection");
    let store = MemoryStore::open_default().expect("unprotected memories");
    store
        .write(
            Path::new("fact.md"),
            "# Fact\n\nRust is preferred.\n",
            false,
        )
        .expect("note");
    let server = MemoryMcp::new(store.clone());
    crate::protection::lock().expect("lock app");
    assert!(
        store.read(Path::new("fact.md")).is_err(),
        "app entry lock protects every content route"
    );
    assert!(
        server
            .memory_read(rmcp::handler::server::wrapper::Parameters(
                super::ReadParams {
                    path: "fact.md".to_string()
                },
            ))
            .is_err(),
        "headless MCP must enforce the app entry password"
    );
}
