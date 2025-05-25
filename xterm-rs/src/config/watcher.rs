use anyhow::{Context, Result};
use notify_debouncer_mini::{
    DebouncedEventKind::{Any, AnyContinuous},
    new_debouncer,
    notify::RecursiveMode,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::models::AppConfig;

async fn read_cfg(path: &Path) -> Result<AppConfig> {
    let txt = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("read {:?}", path))?;
    Ok(toml::from_str(&txt)?)
}

#[derive(Clone)]
pub struct ConfigWatcher {
    inner: Arc<watch::Sender<AppConfig>>,
}

impl ConfigWatcher {
    pub fn current(&self) -> AppConfig {
        self.inner.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<AppConfig> {
        self.inner.subscribe()
    }
}

pub async fn spawn_cfg_watcher(path: PathBuf) -> Result<(ConfigWatcher, JoinHandle<()>)> {
    let init_cfg = read_cfg(&path).await.unwrap_or_else(|_| AppConfig {
        layout: "qwerty".into(),
        theme: "Default".into(),
    });

    let (tx, _rx_cfg) = watch::channel(init_cfg.clone());

    let tx_in_task = tx.clone();
    let path_in_task = path.clone();
    let dir_in_task = path.parent().unwrap().to_path_buf();
    let target_name = path.file_name().unwrap().to_owned();

    let (tx_async, mut rx_async) = mpsc::channel(8);
    let mut debouncer = new_debouncer(Duration::from_millis(50), move |res| {
        if let Ok(events) = res {
            let _ = tx_async.blocking_send(events);
        }
    })?;
    debouncer.watcher().watch(&dir_in_task, RecursiveMode::NonRecursive)?;

    let handle = tokio::spawn(async move {
        while let Some(events) = rx_async.recv().await {
            for ev in events {
                if ev.path.file_name() != Some(&target_name) {
                    continue;
                }
                if !matches!(ev.kind, Any | AnyContinuous) {
                    continue;
                }

                // pause
                let _ = debouncer.watcher().unwatch(&dir_in_task);
                if let Ok(cfg) = read_cfg(&path_in_task).await {
                    let _ = tx_in_task.send(cfg);
                }
                // resume
                let _ = debouncer.watcher().watch(&dir_in_task, RecursiveMode::NonRecursive);
            }
        }
    });

    Ok((ConfigWatcher { inner: Arc::new(tx) }, handle))
}
