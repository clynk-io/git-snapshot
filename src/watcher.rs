use notify::{
    Config, Event, EventHandler, PollWatcher, RecommendedWatcher, Watcher as NotifyWatcher,
};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use std::{
    collections::HashMap,
    fs::canonicalize,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::mpsc::unbounded_channel, time::sleep};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "mode", content = "mode_config")]
pub enum WatchMode {
    Event,
    Poll {
        #[serde(with = "humantime_serde")]
        interval: Duration,
    },
}

pub trait Handler {
    fn handle(&mut self, path: PathBuf);
}

impl<F: FnMut(PathBuf) -> ()> Handler for F {
    fn handle(&mut self, path: PathBuf) {
        (self)(path);
    }
}
type BoxedNotifyWatcher = Box<dyn NotifyWatcher + Send + Sync>;

pub struct Watcher {
    notify_watcher: BoxedNotifyWatcher,
    handlers: Arc<Mutex<HashMap<PathBuf, Box<dyn Handler + Send + Sync>>>>,
}

impl Watcher {
    fn notify_watcher(
        mode: &WatchMode,
        handler: impl EventHandler,
    ) -> Result<BoxedNotifyWatcher, Error> {
        let watcher: BoxedNotifyWatcher = match mode {
            WatchMode::Event => {
                let watcher = RecommendedWatcher::new(handler, Config::default())?;
                Box::new(watcher)
            }
            WatchMode::Poll { interval } => {
                let watcher = PollWatcher::new(
                    handler,
                    Config::default().with_poll_interval(interval.clone()),
                )?;
                Box::new(watcher)
            }
        };
        Ok(watcher)
    }

    pub fn new(mode: &WatchMode, debounce_period: Duration) -> Result<Self, Error> {
        let handlers: Arc<Mutex<HashMap<PathBuf, Box<dyn Handler + Send + Sync>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = unbounded_channel::<PathBuf>();
        let handler = move |event: Result<Event, notify::Error>| -> () {
            if let Ok(event) = event {
                if !event.kind.is_create() && !event.kind.is_modify() && !event.kind.is_remove() {
                    return;
                }

                for event_path in &event.paths {
                    let _ = tx.send(event_path.clone());
                }
            }
        };

        let handlers_clone = handlers.clone();

        tokio::spawn(async move {
            while let Some(event_path) = rx.recv().await {
                let handlers = handlers_clone.lock().unwrap();
                let mut debouncers = HashMap::new();

                for p in handlers.keys() {
                    if event_path.starts_with(p.as_path()) {
                        let handler_path = p.clone();
                        let handlers = handlers_clone.clone();
                        let debounce_period = debounce_period.clone();

                        let join_handle = tokio::spawn(async move {
                            sleep(debounce_period).await;
                            if let Some(handler) = handlers.lock().unwrap().get_mut(&handler_path) {
                                handler.handle(handler_path);
                            }
                        });

                        // abort the existing handle for debouncing
                        if let Some(old_handle) = debouncers.insert(p.clone(), join_handle) {
                            old_handle.abort();
                        }
                        break;
                    }
                }
            }
        });

        let notify_watcher = Self::notify_watcher(&mode, handler)?;

        Ok(Self {
            notify_watcher,
            handlers,
        })
    }

    pub fn watch_path(
        &mut self,
        path: impl AsRef<Path>,
        handler: Box<dyn Handler + Send + Sync>,
    ) -> Result<(), Error> {
        let path = canonicalize(path)?;
        self.notify_watcher
            .watch(&path, notify::RecursiveMode::Recursive)?;

        self.handlers.lock().unwrap().insert(path, handler);

        Ok(())
    }

    pub fn unwatch_path(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = canonicalize(path).unwrap();
        self.notify_watcher.unwatch(&path)?;
        self.handlers.lock().unwrap().remove(&path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::{tempdir, NamedTempFile};
    use tokio::sync::mpsc::UnboundedReceiver;

    use super::*;

    fn test_watcher(path: &Path, mode: &WatchMode) -> (Watcher, UnboundedReceiver<PathBuf>) {
        let mut watcher = Watcher::new(mode, Duration::from_millis(100)).unwrap();
        let (tx, rx) = unbounded_channel();
        watcher
            .watch_path(
                path,
                Box::new(move |p: PathBuf| {
                    let _ = tx.send(p);
                }),
            )
            .unwrap();
        (watcher, rx)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn event_watcher() {
        let root = tempdir().unwrap();
        let root_path = canonicalize(root.path()).unwrap();
        let (_watcher, mut rx) = test_watcher(root.path(), &WatchMode::Event);
        NamedTempFile::new_in(root.path()).unwrap().keep().unwrap();

        let item = rx.recv().await;
        assert!(item.is_some());
        assert_eq!(item.unwrap(), root_path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn poll_watcher() {
        let root = tempdir().unwrap();
        let root_path = canonicalize(root.path()).unwrap();
        let (_watcher, mut rx) = test_watcher(
            root.path(),
            &WatchMode::Poll {
                interval: Duration::from_millis(10),
            },
        );
        NamedTempFile::new_in(root.path()).unwrap().keep().unwrap();

        let item = rx.recv().await;
        assert!(item.is_some());
        assert_eq!(item.unwrap(), root_path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn debounce() {
        let root = tempdir().unwrap();
        let root_path = canonicalize(root.path()).unwrap();
        let (_watcher, mut rx) = test_watcher(root.path(), &WatchMode::Event);

        NamedTempFile::new_in(root.path()).unwrap().keep().unwrap();
        sleep(Duration::from_millis(50)).await;

        NamedTempFile::new_in(root.path()).unwrap().keep().unwrap();
        assert!(rx.try_recv().is_err());

        sleep(Duration::from_millis(100)).await;

        let item = rx.recv().await;
        assert!(item.is_some());
        assert_eq!(item.unwrap(), root_path);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unwatch() {
        let root = tempdir().unwrap();
        let root_path = canonicalize(root.path()).unwrap();
        let (mut watcher, mut rx) = test_watcher(root.path(), &WatchMode::Event);

        watcher.unwatch_path(&root_path).unwrap();

        NamedTempFile::new_in(root.path()).unwrap().keep().unwrap();

        assert!(rx.recv().await.is_none());
    }
}
