use crate::error::Error;
use git2::{Config, Cred, Index, IndexAddOption, PushOptions, RemoteCallbacks, Repository};
use log::{debug, info};
use shellexpand::env_with_context_no_errors;

use std::path::Path;


const BRANCH_SUB_KEY: &'static str = "BRANCH";
const DEFAULT_SNAPSHOT_BRANCH: &'static str = "snapshot/${BRANCH}";
const DEFAULT_SNAPSHOT_COMMIT_MESSAGE: &'static str = "Snapshot";
const BRANCH_REF_PREFIX: &'static str = "refs/heads/";

pub struct Repo {
    git_repo: Repository,
}

// trait to easily find the first populated key in git config
trait ConfigValue {
    fn from_config(config: &Config, keys: &[&str], default_value: Self) -> Self
    where
        Self: Sized;
}

impl ConfigValue for String {
    fn from_config(config: &Config, keys: &[&str], default_value: Self) -> Self
    where
        Self: Sized,
    {
        for &key in keys {
            if let Ok(value) = config.get_string(key) {
                return value;
            }
        }
        default_value
    }
}

impl ConfigValue for bool {
    fn from_config(config: &Config, keys: &[&str], default_value: Self) -> Self
    where
        Self: Sized,
    {
        for &key in keys {
            if let Ok(value) = config.get_bool(key) {
                return value;
            }
        }
        default_value
    }
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

        // Check if snapshotting is enabled for the current branch
        let enabled = config
            .get_bool(&format!("branch.{}.snapshotenabled", current_branch))
            .unwrap_or(true);

        if !enabled {
            info!("Skipping snapshot for branch: {}", current_branch);
            return Ok(());
        }

        info!("Snapshotting branch: {}", current_branch);

        let snapshot_branch = String::from_config(
            &config,
            &[
                &format!("branch.{}.snapshotbranch", current_branch),
                "snapshot.snapshotbranch",
            ],
            DEFAULT_SNAPSHOT_BRANCH.to_owned(),
        );

        let snapshot_branch = expand(&snapshot_branch, &[(BRANCH_SUB_KEY, &current_branch)]);

        debug!("Snapshot branch: {}", snapshot_branch);
        // create full branch ref name, e.g. refs/heads/snapshot/main
        let snapshot_ref_name = [BRANCH_REF_PREFIX, &snapshot_branch].concat();

        // Build the index with the current local changes and write to repo
        let mut index = Index::new()?;
        self.git_repo.set_index(&mut index)?;
        index.add_all(&["*"], IndexAddOption::DEFAULT, None)?;
        if index.is_empty() {
            return Ok(());
        }
        let tree = index.write_tree()?;
        let tree = self.git_repo.find_tree(tree)?;

        // Get the current reference to the destination snapshot branch for diffing and the commit parent
        let snapshot_ref = self.git_repo.find_reference(&snapshot_ref_name).ok();

        // Diff the current index to the previous snapshot commit tree to check for changes
        let diff = self.git_repo.diff_tree_to_tree(
            snapshot_ref
                .as_ref()
                .and_then(|r| r.peel_to_tree().ok())
                .as_ref(),
            Some(&tree),
            None,
        )?;
        if diff.deltas().next().is_none() {
            info!("No changes from previous snapshot, aborting snapshot");
            return Ok(());
        }

        let signature = self.git_repo.signature()?;

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
        //let config = Arc::new(config);

        let remotes = self.git_repo.remotes()?;

        for remote in &remotes {
            let remote = remote.unwrap();
            let enabled = bool::from_config(
                &config,
                &[&format!("remote.{}.snapshotenabled", remote)],
                false,
            );

            if !enabled {
                debug!("Snapshots disabled for remote: {}, skipping", remote);
                continue;
            }

            info!("Pushing snapshot to remote: {}", remote);

            let snapshot_branch = String::from_config(
                &config,
                &[&format!("remote.{}.snapshotbranch", remote)],
                ref_name.trim_start_matches(BRANCH_REF_PREFIX).to_owned(),
            );

            let snapshot_ref_name = [BRANCH_REF_PREFIX, &snapshot_branch].concat();

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
            remote.push(&[[ref_name, &snapshot_ref_name].join(":")], Some(&mut opts))?;
        }

        Ok(())
    }

    fn current_branch(&self) -> Result<String, Error> {
        let reference = self.git_repo.head()?;

        // Return an error if head doesn't point to a branch
        if !reference.is_branch() {
            return Err(Error::InvalidHead);
        }
        Ok(reference.shorthand().unwrap().to_owned())
    }
}
