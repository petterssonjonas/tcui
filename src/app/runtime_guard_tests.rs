use super::Action;

#[test]
fn update_check_queues_a_toast_when_a_tokio_runtime_finds_a_new_release() {
    super::tests::with_test_app("update-check-runtime", |app| {
        // Given
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        let action = runtime.block_on(async {
            app.queue_update_check_with(async {
                Ok::<_, color_eyre::Report>(Some(crate::updater::ReleaseInfo {
                    tag: "v999.0.0".to_string(),
                    version: "999.0.0".to_string(),
                    asset_name: "tcui.tar.gz".to_string(),
                    asset_url: "https://example.test/tcui.tar.gz".to_string(),
                    sums_url: "https://example.test/SHA256SUMS".to_string(),
                }))
            });
            tokio::time::timeout(std::time::Duration::from_secs(1), app.action_rx.recv())
                .await
                .expect("receive queued update action")
        });

        // Then
        assert!(matches!(
            action,
            Some(Action::ShowToast(message))
                if message == "Update 999.0.0 available. Run `tcui upgrade` to update."
        ));
    });
}

#[test]
fn connection_check_queues_a_checking_state_when_a_tokio_runtime_is_available() {
    super::tests::with_test_app("connection-check-runtime", |app| {
        // Given
        let runtime = tokio::runtime::Runtime::new().expect("create runtime");

        // When
        let action = runtime.block_on(async {
            app.queue_connection_check_for_active_tab();
            tokio::time::timeout(std::time::Duration::from_secs(1), app.action_rx.recv())
                .await
                .expect("receive queued connection action")
        });

        // Then
        assert!(matches!(
            action,
            Some(Action::SetConnectionState(
                crate::ui::status_bar::ConnectionStatus::Checking,
                Some(message),
            )) if message == "Checking connection..."
        ));
    });
}
