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

        let signature = Signature::now("asdf", "asdf@asdg.com")?;

        let parent = self
            .git_repo
            .find_reference(&snapshot_ref)
            .and_then(|r| r.peel_to_commit())
            .ok();

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
            //config.add_file(&git2::Config::find_global()?, ConfigLevel::Global, false)?;
            //config.add_file(&git2::Config::find_system()?, ConfigLevel::System, false)?;

            callbacks.credentials(move |url, username, allowed_types| {
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
