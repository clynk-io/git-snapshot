use crate::config::{Config, Remote};
use crate::error::Error;
use git2::{
    ConfigLevel, Cred, CredentialHelper, Index, IndexAddOption, PushOptions, RemoteCallbacks,
    Repository, Signature,
};
use std::path::Path;

pub struct Repo {
    git_repo: Repository,
    config: Config,
}

impl Repo {
    pub fn new(path: impl AsRef<Path>, config: Config) -> Result<Self, Error> {
        let git_repo = Repository::discover(path)?;
        Ok(Self { git_repo, config })
    }

    pub fn snapshot(&self) -> Result<(), Error> {
        let current_branch = self.current_branch()?;

        let snapshot_ref = format!("refs/heads/snapshots/{}", current_branch);
        let mut index = Index::new()?;
        self.git_repo.set_index(&mut index)?;
        index.add_all(&["*"], IndexAddOption::DEFAULT, None)?;
        if index.is_empty() {
            return Ok(());
        }
        let tree = index.write_tree()?;
        let tree = self.git_repo.find_tree(tree)?;

        let snapshot_reference = self.git_repo.find_reference(&snapshot_ref).ok();

        let diff = self.git_repo.diff_tree_to_tree(
            snapshot_reference
                .as_ref()
                .and_then(|r| r.peel_to_tree().ok())
                .as_ref(),
            Some(&tree),
            None,
        )?;
        if diff.deltas().next().is_none() {
            return Ok(());
        }

        let signature = Signature::now("asdf", "asdf@asdg.com")?;

        let parent = snapshot_reference.and_then(|r| r.peel_to_commit().ok());
        self.git_repo.commit(
            Some(&snapshot_ref),
            &signature,
            &signature,
            "snapshot",
            &tree,
            parent
                .as_ref()
                .as_ref()
                .map(std::slice::from_ref)
                .unwrap_or_default(),
        )?;

        for remote in &self.config.remotes {
            let mut remote = self.git_repo.find_remote(&remote.name)?;

            let mut callbacks = RemoteCallbacks::new();
            let mut config = self.git_repo.config()?;
            if let Ok(p) = git2::Config::find_global() {
                config.add_file(&p, ConfigLevel::Global, true)?;
            }
            if let Ok(p) = git2::Config::find_system() {
                config.add_file(&p, ConfigLevel::System, true)?;
            }
            for entry in &config.entries(None).unwrap() {
                let entry = entry.unwrap();
                println!("{} => {}", entry.name().unwrap(), entry.value().unwrap());
            }
            callbacks.credentials(move |url, username, allowed_types| {
                println!("{:?}", allowed_types);
                Cred::credential_helper(&config, url, username)
            });
            let mut opts = PushOptions::new();
            opts.remote_callbacks(callbacks);
            remote.push(
                &[format!("{}:{}", snapshot_ref, snapshot_ref)],
                Some(&mut opts),
            )?;
        }
        Ok(())
    }

    fn current_branch(&self) -> Result<String, Error> {
        let reference = self.git_repo.head()?;
        if !reference.is_branch() {
            return Err(Error::InvalidHead);
        }
        Ok(reference.shorthand().unwrap().to_owned())
    }
}
