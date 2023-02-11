use super::*;
use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::{Mutex, RawMutex};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::trace;

#[derive(Debug)]
pub struct DynamicConfig<T>(pub(crate) Arc<Mutex<Config<T>>>);

pub struct DynamicConfigWatcher<T, W> {
    pub(crate) inner: Arc<Mutex<Config<T>>>,
    pub(crate) inner_watcher: W,
    pub(crate) handle: Option<JoinHandle<()>>,
    pub(crate) topic: broadcast::Sender<Raw>,
}

pub struct FileWatcher {
    path: PathBuf,
    watcher: RecommendedWatcher,
}

impl<T> DynamicConfig<T>
where
    T: Clone,
{
    pub fn as_arc(&self) -> Arc<Mutex<Config<T>>> {
        Arc::clone(&self.0)
    }

    pub fn into_arc(self) -> Arc<Mutex<Config<T>>> {
        self.0
    }

    pub fn lock(&self) -> parking_lot::lock_api::MutexGuard<'_, RawMutex, Config<T>> {
        self.0.lock()
    }

    pub fn to_inner(&self) -> T {
        self.0.lock().clone().0
    }
}

impl<T> DynamicConfig<T>
where
    T: serde::de::DeserializeOwned,
{
    pub fn watch_file(path: impl AsRef<Path>) -> (Self, DynamicConfigWatcher<T, FileWatcher>) {
        let watch_path = path.as_ref();
        let path = path.as_ref().to_owned();
        let (topic, _) = broadcast::channel(DEFAULT_BUFFER_SIZE);
        let watcher_topic = broadcast::Sender::clone(&topic);
        let watcher = recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                if !event.kind.is_modify() {
                    return;
                }
                let raw = std::fs::read_to_string(&path).ok();
                if let Some(raw_str) = raw {
                    let typ = parse_type(&path);
                    let _ = watcher_topic.send(Raw { raw_str, typ });
                }
            }
        })
        .unwrap();
        let config = Arc::new(Mutex::new(Config::from_file(watch_path)));
        let config_watcher = DynamicConfigWatcher {
            inner: Arc::clone(&config),
            inner_watcher: FileWatcher {
                path: watch_path.to_owned(),
                watcher,
            },
            handle: None,
            topic,
        };
        let dynamic_config = Self(config);
        (dynamic_config, config_watcher)
    }
}

pub trait InnerWatcher {
    fn watch(&mut self) -> Result<(), Error>;
    fn stop(&mut self) -> Result<(), Error>;
}

impl<T, W> InnerWatcher for DynamicConfigWatcher<T, W>
where
    T: serde::de::DeserializeOwned + Clone + Sync + Send + 'static,
    W: InnerWatcher + Send,
{
    fn watch(&mut self) -> Result<(), Error> {
        self.stop()?;
        let mut rx = self.topic.subscribe();
        let inner = Arc::clone(&self.inner);
        let handle = tokio::spawn(async move {
            loop {
                if let Ok(new) = rx.recv().await {
                    let new = new.try_into().unwrap_or_else(|_| inner.lock().clone());
                    *inner.lock() = new;
                    trace!("[KOSEI] updated dynamic configuration")
                }
            }
        });
        self.handle = Some(handle);
        self.inner_watcher.watch()
    }

    fn stop(&mut self) -> Result<(), Error> {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
        self.inner_watcher.stop()
    }
}

impl InnerWatcher for FileWatcher {
    fn watch(&mut self) -> Result<(), Error> {
        self.watcher.watch(&self.path, RecursiveMode::Recursive)?;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        match self.watcher.unwatch(&self.path) {
            Ok(_) => Ok(()),
            Err(e) => {
                if matches!(e.kind, notify::ErrorKind::WatchNotFound) {
                    return Ok(());
                }
                Err(e.into())
            }
        }
    }
}
