#![allow(dead_code)]
use color_eyre::Result;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;

pub struct VaultWatcher {
    watcher: Option<RecommendedWatcher>,
}

impl VaultWatcher {
    pub fn new<F>(_path: &Path, callback: F) -> Result<Self>
    where
        F: Fn(notify::Event) + Send + 'static,
    {
        let watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    callback(event);
                }
            },
            Config::default(),
        )?;

        Ok(Self {
            watcher: Some(watcher),
        })
    }

    pub fn watch(&mut self, path: &Path) -> Result<()> {
        if let Some(ref mut watcher) = self.watcher {
            watcher.watch(path, RecursiveMode::Recursive)?;
        }
        Ok(())
    }
}
