use crate::config::{Config, Remote};
use crate::error::Error;
use git2::{
    ConfigLevel, Cred, CredentialHelper, Index, IndexAddOption, PushOptions, RemoteCallbacks,
    Repository, Signature,
};
use std::path::Path;
use std::sync::Arc;

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

        self.push(&snapshot_ref)
    }

    fn push(&self, ref_name: &str) -> Result<(), Error> {
        let config = Arc::new(self.git_repo.config()?);
        let remote_entries = config.multivar("snapshot.remote", None)?;
        let remote_entries = remote_entries.filter_map(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.value().map(|e| e.to_owned()))
        });

        for remote in remote_entries {
            let mut remote = self.git_repo.find_remote(&remote)?;
            let config = config.clone();
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(move |url, username, allowed_types| {
                if allowed_types.is_user_pass_plaintext() {
                    if let Ok(cred) = Cred::credential_helper(&config, url, username) {
                        return Ok(cred);
                    }
                }
                if allowed_types.is_ssh_key() {
                    if let Some(username) = username {
                        if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                            return Ok(cred);
                        }
                    }
                }
                Err(git2::Error::new(
                    git2::ErrorCode::Auth,
                    git2::ErrorClass::Callback,
                    "unable to authenticate",
                ))
            });
            let mut opts = PushOptions::new();
            opts.remote_callbacks(callbacks);
            remote.push(&[format!("{}:{}", ref_name, ref_name)], Some(&mut opts))?;
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
