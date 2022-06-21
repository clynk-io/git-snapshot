use git_snapshot::repo_watcher::{RepoWatcher, WatchConfig};

use git_snapshot::Repo;
use log::{error, LevelFilter};
use serde_json::{from_reader, to_writer};
use structopt::StructOpt;

use anyhow::{anyhow, Error};

use std::env::current_dir;
use std::fmt::Display;
use std::fs::{create_dir_all, OpenOptions};
use std::io::ErrorKind;
use std::str::FromStr;

use pretty_env_logger::formatted_builder;
use std::path::{Path, PathBuf};
use std::thread::park;

fn default_config_path() -> Result<PathBuf, Error> {
    let home = dirs::home_dir().ok_or(anyhow!("Unable to get home directory"))?;
    Ok(home.join(
        [".config", "git-snapshot", "config.json"]
            .iter()
            .collect::<PathBuf>(),
    ))
}

#[derive(Debug)]
enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
}
impl Default for LogLevel {
    fn default() -> Self {
        Self::Info
    }
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Off => write!(f, "off"),
            Self::Error => write!(f, "error"),
            Self::Warn => write!(f, "warn"),
            Self::Info => write!(f, "info"),
            Self::Debug => write!(f, "debug"),
        }
    }
}

impl FromStr for LogLevel {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "off" => Ok(LogLevel::Off),
            "error" => Ok(LogLevel::Error),
            "warn" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            _ => Err(anyhow!("Invalid log level: {}", s)),
        }
    }
}

impl LogLevel {
    fn to_level_filter(&self) -> LevelFilter {
        match self {
            Self::Off => LevelFilter::Off,
            Self::Error => LevelFilter::Error,
            Self::Warn => LevelFilter::Warn,
            Self::Info => LevelFilter::Info,
            Self::Debug => LevelFilter::Debug,
        }
    }
}

#[derive(Debug, StructOpt)]
#[structopt(name = "git-snapshot", about = "Automate snapshots for git")]
struct App {
    #[structopt(subcommand)]
    cmds: Option<AppCommands>,
    #[structopt(default_value, short, long, env = "GIT_SNAPSHOT_LOG_LEVEL")]
    log_level: LogLevel,
}

#[derive(Debug, StructOpt)]
enum AppCommands {
    Watch {
        #[structopt(short, long, env = "GIT_SNAPSHOT_CONFIG")]
        config: Option<PathBuf>,
        path: PathBuf,
    },
    Unwatch {
        #[structopt(short, long, env = "GIT_SNAPSHOT_CONFIG")]
        config: Option<PathBuf>,
        path: PathBuf,
    },
    StartWatcher {
        #[structopt(short, long, env = "GIT_SNAPSHOT_CONFIG")]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() {
    let app = App::from_args();
    formatted_builder()
        .filter_level(app.log_level.to_level_filter())
        .format_module_path(false)
        .init();
    if let Err(err) = run(app) {
        error!("{:?}", err)
    }
}

fn run(app: App) -> Result<(), Error> {
    if let Some(cmds) = app.cmds {
        match cmds {
            AppCommands::StartWatcher { config } => {
                let _watcher = RepoWatcher::with_config(config.unwrap_or(default_config_path()?))?;
                park();
            }
            AppCommands::Watch { config, path } => {
                let p = config.unwrap_or(default_config_path()?);
                let mut config = load_config(&p)?;
                config.add_repo(path)?;
                save_config(&p, &config)?;
            }
            AppCommands::Unwatch { config, path } => {
                let p = config.unwrap_or(default_config_path()?);
                let mut config = load_config(&p)?;
                config.remove_repo(path)?;
                save_config(&p, &config)?;
            }
        }
    } else {
        let cwd = current_dir()?;
        let repo = Repo::from_path(cwd)?;
        repo.snapshot()?;
    }
    Ok(())
}

fn load_config(p: &Path) -> Result<WatchConfig, Error> {
    match OpenOptions::new().read(true).open(p) {
        Ok(f) => from_reader(f).map_err(From::from),
        Err(err) => {
            if err.kind() == ErrorKind::NotFound {
                Ok(WatchConfig::default())
            } else {
                Err(err.into())
            }
        }
    }
}

fn save_config(p: &Path, config: &WatchConfig) -> Result<(), Error> {
    create_dir_all(p.parent().ok_or(anyhow!("Invalid config path"))?)?;
    let f = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(p)?;
    to_writer(f, config).map_err(From::from)
}
