use notify::{
    poll::PollWatcherConfig, Event, EventHandler, PollWatcher, RecommendedWatcher,
    Watcher as NotifyWatcher,
};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Debug, Deserialize, Serialize)]
pub enum WatchMode {
    Event,
    Poll,
}

pub trait Handler {
    fn handle(&mut self, path: PathBuf, handler_path: PathBuf);
}

impl<F: FnMut(PathBuf, PathBuf) -> ()> Handler for F {
    fn handle(&mut self, path: PathBuf, handler_path: PathBuf) {
        (self)(path, handler_path);
    }
}
type BoxedNotifyWatcher = Box<dyn NotifyWatcher + Send + Sync>;

pub struct Watcher {
    notify_watcher: BoxedNotifyWatcher,
    paths: Arc<Mutex<Vec<(PathBuf, Box<dyn Handler + Send + Sync>)>>>,
}

impl Watcher {
    fn notify_watcher(
        mode: &WatchMode,
        period: Duration,
        handler: impl EventHandler,
    ) -> Result<BoxedNotifyWatcher, Error> {
        let watcher: BoxedNotifyWatcher = match mode {
            WatchMode::Event => {
                let watcher = RecommendedWatcher::new(handler)?;
                Box::new(watcher)
            }
            WatchMode::Poll => {
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

    pub fn new(mode: &WatchMode, period: Duration) -> Result<Self, Error> {
        let paths: Arc<Mutex<Vec<(PathBuf, Box<dyn Handler + Send + Sync>)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let paths_clone = paths.clone();
        let handler = move |event: Result<Event, notify::Error>| -> () {
            if let Ok(event) = event {
                if !event.kind.is_create() && !event.kind.is_modify() && !event.kind.is_remove() {
                    return;
                }
                let mut paths = paths_clone.lock().unwrap();
                for (p, handler) in &mut *paths {
                    for event_path in &event.paths {
                        if event_path.starts_with(p.as_path()) {
                            handler.handle(event_path.clone(), p.clone())
                        }
                    }
                }
            }
        };
        let notify_watcher = Self::notify_watcher(&mode, period, handler)?;
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
        self.paths.lock().unwrap().push((path, handler));
        Ok(())
    }
}
