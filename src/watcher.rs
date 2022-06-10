use notify::{
    poll::PollWatcherConfig, Event, EventHandler, PollWatcher, RecommendedWatcher,
    Watcher as NotifyWatcher,
};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::mpsc::unbounded_channel, time::sleep};

#[derive(Debug, Deserialize, Serialize)]
pub enum WatchMode {
    Event,
    Poll { period: Duration },
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
    paths: Arc<Mutex<HashMap<PathBuf, Box<dyn Handler + Send + Sync>>>>,
}

impl Watcher {
    fn notify_watcher(
        mode: &WatchMode,
        handler: impl EventHandler,
    ) -> Result<BoxedNotifyWatcher, Error> {
        let watcher: BoxedNotifyWatcher = match mode {
            WatchMode::Event => {
                let watcher = RecommendedWatcher::new(handler)?;
                Box::new(watcher)
            }
            WatchMode::Poll { period } => {
                let watcher = PollWatcher::with_config(
                    handler,
                    PollWatcherConfig {
                        poll_interval: period.clone(),
                        compare_contents: false,
                    },
                )?;
                Box::new(watcher)
            }
        };
        Ok(watcher)
    }

    pub fn new(mode: &WatchMode, debounce_period: Duration) -> Result<Self, Error> {
        let paths: Arc<Mutex<HashMap<PathBuf, Box<dyn Handler + Send + Sync>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = unbounded_channel::<PathBuf>();
        let handler = move |event: Result<Event, notify::Error>| -> () {
            if let Ok(event) = event {
                if !event.kind.is_create() && !event.kind.is_modify() && !event.kind.is_remove() {
                    return;
                }

                for event_path in &event.paths {
                    tx.send(event_path.clone());
                }
            }
        };

        let paths_clone = paths.clone();

        tokio::spawn(async move {
            while let Some(event_path) = rx.recv().await {
                let paths = paths_clone.lock().unwrap();
                let mut debouncers = HashMap::new();

                for p in paths.keys() {
                    if event_path.starts_with(p.as_path()) {
                        let handler_path = p.clone();
                        let handlers = paths_clone.clone();
                        let debounce_period = debounce_period.clone();

                        let join_handle = tokio::spawn(async move {
                            sleep(debounce_period).await;
                            if let Some(handler) = handlers.lock().unwrap().get_mut(&handler_path) {
                                handler.handle(handler_path);
                            }
                        });

                        if let Some(old_handle) = debouncers.insert(p.clone(), join_handle) {
                            old_handle.abort();
                        }
                    }
                }
            }
        });

        let notify_watcher = Self::notify_watcher(&mode, handler)?;

        Ok(Self {
            notify_watcher,
            paths,
        })
    }

    pub fn watch_path(
        &mut self,
        path: impl Into<PathBuf>,
        handler: Box<dyn Handler + Send + Sync>,
    ) -> Result<(), Error> {
        let path = path.into();
        self.notify_watcher
            .watch(&path, notify::RecursiveMode::Recursive)?;

        self.paths.lock().unwrap().insert(path, handler);

        Ok(())
    }
}
