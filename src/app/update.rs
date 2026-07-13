use super::{Action, TuiApp};

impl TuiApp {
    pub fn queue_update_check(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        self.queue_update_check_with(crate::updater::available_release());
    }

    pub(super) fn queue_update_check_with(
        &self,
        release_check: impl std::future::Future<
            Output = color_eyre::Result<Option<crate::updater::ReleaseInfo>>,
        > + Send
        + 'static,
    ) {
        let action_tx = self.action_tx.clone();
        tokio::spawn(async move {
            if let Ok(Some(release)) = release_check.await {
                let _ = action_tx.send(Action::ShowToast(format!(
                    "Update {} available. Run `tcui upgrade` to update.",
                    release.version
                )));
            }
        });
    }
}
