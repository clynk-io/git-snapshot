use crate::error::Error;
use git2::{
    Config, ConfigLevel, Cred, CredentialHelper, Index, IndexAddOption, PushOptions,
    RemoteCallbacks, Repository, Signature,
};
use shellexpand::{env_with_context, env_with_context_no_errors};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

const BRANCH_SUB_KEY: &'static str = "BRANCH";
const DEFAULT_SNAPSHOT_BRANCH: &'static str = "snapshot/${BRANCH}";

pub struct Repo {
    git_repo: Repository,
}

fn expand(input: &str, context: &[(&str, &str)]) -> String {
    env_with_context_no_errors(input, |var| {
        for &(key, val) in context {
            if var == key {
                return Some(val.to_owned());
            }
        }
        None
    })
    .to_string()
}

impl Repo {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, Error> {
        let git_repo = Repository::discover(path)?;
        Ok(Self { git_repo })
    }

    pub fn snapshot(&self) -> Result<(), Error> {
        let current_branch = self.current_branch()?;

        let config = self.git_repo.config()?;

        let enabled = config
            .get_bool(&format!("branch.{}.snapshotenabled", current_branch))
            .unwrap_or(true);

        if !enabled {
            return Ok(());
        }

        let snapshot_branch =
            config.get_string(&format!("branch.{}.snapshotbranch", current_branch));

        let snapshot_branch = snapshot_branch
            .or(config.get_string("snapshot.snapshotbranch"))
            .unwrap_or(DEFAULT_SNAPSHOT_BRANCH.to_owned());

        let snapshot_branch = expand(&snapshot_branch, &[(BRANCH_SUB_KEY, &current_branch)]);

        let snapshot_ref_name = format!("refs/heads/{}", snapshot_branch);

        let mut index = Index::new()?;
        self.git_repo.set_index(&mut index)?;
        index.add_all(&["*"], IndexAddOption::DEFAULT, None)?;
        if index.is_empty() {
            return Ok(());
        }
        let tree = index.write_tree()?;
        let tree = self.git_repo.find_tree(tree)?;

        let snapshot_ref = self.git_repo.find_reference(&snapshot_ref_name).ok();

        let diff = self.git_repo.diff_tree_to_tree(
            snapshot_ref
                .as_ref()
                .and_then(|r| r.peel_to_tree().ok())
                .as_ref(),
            Some(&tree),
            None,
        )?;
        if diff.deltas().next().is_none() {
            return Ok(());
        }

        let signature = Signature::now("asdf", "asdf@asdf.com")?;

        let parent = snapshot_ref.and_then(|r| r.peel_to_commit().ok());
        self.git_repo.commit(
            Some(&snapshot_ref_name),
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

        self.push(&snapshot_ref_name, &current_branch, &config)
    }

    fn push(&self, ref_name: &str, current_branch: &str, config: &Config) -> Result<(), Error> {
        let config = Arc::new(config);

        let remotes = self.git_repo.remotes()?;

        for remote in &remotes {
            let remote = remote.unwrap();
            let enabled = config
                .get_bool(&format!("remote.{}.snapshotenabled", remote))
                .unwrap_or(false);
            if !enabled {
                continue;
            }

            let snapshot_branch = config.get_string(&format!("remote.{}.snapshotbranch", remote));

            let snapshot_ref_name = snapshot_branch
                .map(|branch| format!("refs/heads/{}", branch))
                .unwrap_or(ref_name.to_owned());
            let snapshot_ref_name = expand(&snapshot_ref_name, &[(BRANCH_SUB_KEY, current_branch)]);

            println!("Pushing to remote: {}", remote);
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
                    "unable to authenticate, setup ssh key agent or credential helper for this remote and username",
                ))
            });

            let mut opts = PushOptions::new();
            opts.remote_callbacks(callbacks);
            remote.push(
                &[format!("{}:{}", ref_name, snapshot_ref_name)],
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
