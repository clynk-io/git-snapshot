use crate::error::Error;
use crate::util::log_err;
use crate::Repo;
use log::error;
use notify::{poll::PollWatcherConfig, PollWatcher, RecommendedWatcher, Watcher as NotifyWatcher};
use notify::{Event, EventHandler, EventKind};
use serde::{Deserialize, Serialize};
use serde_json::from_reader;
use std::collections::HashSet;
use std::fs::canonicalize;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::{fs::OpenOptions, path::PathBuf};

fn default_poll_period() -> Duration {
    Duration::from_secs(5 * 60)
}

fn default_event_debounce_period() -> Duration {
    Duration::from_secs(60)
}

type BoxedNotifyWatcher = Box<dyn NotifyWatcher + Send + Sync>;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", tag = "mode", content = "modeConfig")]
pub enum WatchMode {
    Poll {
        #[serde(with = "humantime_serde", default = "default_poll_period")]
        period: Duration,
    },
    Event {
        #[serde(with = "humantime_serde", default = "default_event_debounce_period")]
        debounce_period: Duration,
    },
}

impl Default for WatchMode {
    fn default() -> Self {
        Self::Event {
            debounce_period: default_event_debounce_period(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoConfig {
    pub path: PathBuf,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchConfig {
    #[serde(default)]
    pub repos: Vec<RepoConfig>,
    #[serde(default, flatten)]
    pub mode: WatchMode,
}

pub struct Watcher {
    config: WatchConfig,
    config_path: Option<PathBuf>,
    watcher: RwLock<Option<Box<dyn NotifyWatcher + Send + Sync>>>,
    channel: (Sender<ChannelMessage>, Receiver<ChannelMessage>),
}

enum ChannelMessage {
    Event(Result<Event, notify::Error>),
    Stop,
}
impl Watcher {
    pub fn from_config_path(config_path: impl Into<PathBuf>) -> Result<Self, Error> {
        let config_path = config_path.into();
        let f = OpenOptions::new().read(true).open(&config_path)?;
        let config: WatchConfig = from_reader(f)?;
        Ok(Self {
            config_path: Some(config_path),
            config: config,
            channel: channel(),
            watcher: RwLock::new(None),
        })
    }

    pub fn new(config: WatchConfig) -> Self {
        Self {
            config: config,
            config_path: None,
            channel: channel(),
            watcher: RwLock::new(None),
        }
    }

    fn load_config(path: &Path) -> Result<WatchConfig, Error> {
        let f = OpenOptions::new().read(true).open(path)?;
        from_reader(f).map_err(From::from)
    }

    fn init_watcher(
        &self,
        tx: Sender<ChannelMessage>,
    ) -> Result<Box<dyn NotifyWatcher + Send + Sync>, Error> {
        let mut watcher = Self::notify_watcher(&self.config.mode, tx)?;
        for r in &self.config.repos {
            watcher.watch(&r.path, notify::RecursiveMode::Recursive)?;
        }
        Ok(watcher)
    }

    pub fn watch(&mut self) -> Result<(), Error> {
        let watcher = self.init_watcher(self.channel.0.clone())?;
        self.watcher.write().unwrap().replace(watcher);

        for msg in &self.channel.1 {
            match msg {
                ChannelMessage::Stop => {
                    self.watcher.write().unwrap().take();
                    break;
                }
                ChannelMessage::Event(event) => {
                    println!("Event: {:?}", event);
                    if let Ok(event) = log_err(event) {
                        if event.kind.is_create()
                            || event.kind.is_modify()
                            || event.kind.is_remove()
                        {
                            let mut paths = HashSet::new();
                            let mut reload_config = false;
                            for p in &event.paths {
                                for repo in &self.config.repos {
                                    if p.starts_with(&repo.path) {
                                        paths.insert(repo.path.as_path());
                                    }
                                }
                                if let Some(config_path) = self.config_path.as_deref() {
                                    if !reload_config && p == config_path {
                                        reload_config = true;
                                    }
                                }
                            }
                            for p in paths {
                                log_err(Repo::from_path(p).and_then(|r| r.snapshot()));
                            }
                            if reload_config {
                                if let Ok(config) =
                                    Self::load_config(self.config_path.as_deref().unwrap())
                                {
                                    self.config = config;
                                    let watcher = self.init_watcher(self.channel.0.clone())?;
                                    self.watcher.write().unwrap().replace(watcher);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn stop(&self) {
        self.channel.0.send(ChannelMessage::Stop);
    }

    fn notify_watcher(
        mode: &WatchMode,
        sender: Sender<ChannelMessage>,
    ) -> Result<Box<dyn NotifyWatcher + Send + Sync>, Error> {
        let handler = move |event| sender.send(ChannelMessage::Event(event)).unwrap();
        let watcher: Box<dyn NotifyWatcher + Send + Sync> = match mode {
            WatchMode::Event { debounce_period } => {
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
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;
    use crate::{tests::check_snapshot_exists, util::tests::*};
    use tempfile::{tempdir, tempdir_in, NamedTempFile};

    fn setup(repo_path: &Path, config: WatchConfig) -> Watcher {
        test_repo(&repo_path);
        Watcher::new(config)
    }

    #[test]
    fn event_watcher() {
        let repo_path = tempdir().unwrap();

        let config = WatchConfig {
            repos: vec![RepoConfig {
                path: repo_path.path().to_owned(),
            }],
            mode: WatchMode::Event {
                debounce_period: Duration::from_secs(60),
            },
        };
        let mut watcher = setup(repo_path.path(), config);

        thread::spawn(move || {
            watcher.watch().unwrap();
        });

        NamedTempFile::new_in(repo_path.path())
            .unwrap()
            .keep()
            .unwrap();
        thread::sleep(Duration::from_millis(2000));
        let repo = Repo::from_path(repo_path.path()).unwrap();

        assert!(check_snapshot_exists(&repo));
    }
}
