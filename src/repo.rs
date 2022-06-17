use crate::error::Error;
use crate::util::{branch_ref_shorthand, expand, ConfigValue, BRANCH_REF_PREFIX};
use git2::{
    Config, Cred, ErrorCode, Index, IndexAddOption, PushOptions, RemoteCallbacks, Repository,
};
use log::{debug, info};
use std::path::Path;

const BRANCH_SUB_KEY: &'static str = "BRANCH";
const DEFAULT_SNAPSHOT_BRANCH: &'static str = "snapshot/${BRANCH}";
const DEFAULT_SNAPSHOT_COMMIT_MESSAGE: &'static str = "Snapshot";

pub struct Repo {
    git_repo: Repository,
}

// TODO: add config setter helper functions
impl Repo {
    pub fn new(repo: Repository) -> Self {
        Repo { git_repo: repo }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let git_repo = Repository::discover(path)?;
        Ok(Self::new(git_repo))
    }

    pub fn git_repo(&self) -> &Repository {
        &self.git_repo
    }

    pub fn name(&self) -> String {
        let mut components = self.git_repo.path().components();
        components
            .next_back()
            .and_then(|c| c.as_os_str().to_str())
            .map(|c| c.to_owned())
            .unwrap_or("unknown".to_owned())
    }

    pub fn snapshot_branch(config: &Config, current_branch: &str) -> String {
        let snapshot_branch = String::from_config(
            &config,
            &[
                &format!("branch.{}.snapshotbranch", current_branch),
                "snapshot.snapshotbranch",
            ],
            DEFAULT_SNAPSHOT_BRANCH.to_owned(),
        );
        expand(&snapshot_branch, &[(BRANCH_SUB_KEY, &current_branch)])
    }

    pub fn snapshot(&self) -> Result<(), Error> {
        let current_branch = self.current_branch()?;
        let config = self.git_repo.config()?;

        // Check if snapshotting is enabled for the current branch
        let enabled = bool::from_config(
            &config,
            &[&format!("branch.{}.snapshotenabled", current_branch)],
            true,
        );

        if !enabled {
            info!("Snapshots disabled for branch: {}", current_branch);
            return Ok(());
        }

        let snapshot_branch = Self::snapshot_branch(&config, &current_branch);

        // create full branch ref name, e.g. refs/heads/snapshot/main
        let snapshot_ref_name = [BRANCH_REF_PREFIX, &snapshot_branch].concat();

        // Build the index with the current local changes and write to repo
        let mut index = Index::new()?;
        self.git_repo.set_index(&mut index)?;
        index.add_all(&["*"], IndexAddOption::DEFAULT, None)?;

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

        // Default signature from config
        let signature = self.git_repo.signature()?;

        let parent = snapshot_ref.and_then(|r| r.peel_to_commit().ok());

        let message = String::from_config(
            &config,
            &[
                &format!("branch.{}.snapshotmessage", current_branch),
                "snapshot.snapshotmessage",
            ],
            DEFAULT_SNAPSHOT_COMMIT_MESSAGE.to_owned(),
        );
        self.git_repo.commit(
            Some(&snapshot_ref_name),
            &signature,
            &signature,
            &message,
            &tree,
            parent
                .as_ref()
                .as_ref()
                .map(std::slice::from_ref)
                .unwrap_or_default(),
        )?;

        info!(
            "Repo: {}, snapshotted branch: {}",
            self.name(),
            current_branch
        );

        self.push(&snapshot_ref_name, &current_branch, &config)
    }

    fn push(&self, ref_name: &str, current_branch: &str, config: &Config) -> Result<(), Error> {
        let remotes = self.git_repo.remotes()?;

        for remote in &remotes {
            let remote = remote.unwrap();

            // Check remote config if snapshots are enabled, disabled by default
            let enabled = bool::from_config(
                &config,
                &[&format!("remote.{}.snapshotenabled", remote)],
                false,
            );

            if !enabled {
                debug!("Snapshots disabled for remote: {}", remote);
                continue;
            }

            // Get remote snapshot branch from remote config or default to the local snapshot branch
            let snapshot_branch = String::from_config(
                &config,
                &[&format!("remote.{}.snapshotbranch", remote)],
                branch_ref_shorthand(ref_name).to_owned(),
            );

            let snapshot_ref_name = [BRANCH_REF_PREFIX, &snapshot_branch].concat();

            let snapshot_ref_name = expand(&snapshot_ref_name, &[(BRANCH_SUB_KEY, current_branch)]);

            let mut remote = self.git_repo.find_remote(&remote)?;

            let config = config.clone();

            let mut callbacks = RemoteCallbacks::new();

            // Only allow non-interactive credentials
            // TODO: Look into using default ssh key
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
            info!(
                "Repo: {}, Pushed snapshot branch to remote: {}",
                self.name(),
                remote.name().unwrap_or("unknown")
            );
        }
        Ok(())
    }

    pub fn current_branch(&self) -> Result<String, Error> {
        match self.git_repo.head() {
            Ok(reference) => {
                if !reference.is_branch() || reference.is_remote() {
                    return Err(Error::InvalidHead);
                }
                reference
                    .shorthand()
                    .map(|r| r.to_owned())
                    .ok_or(Error::InvalidHead)
            }
            Err(err) => {
                if err.code() == ErrorCode::UnbornBranch {
                    let reference = self.git_repo.find_reference("HEAD")?;
                    let target = reference.symbolic_target().ok_or(Error::InvalidHead)?;
                    return Ok(branch_ref_shorthand(target).to_owned());
                }
                Err(Error::InvalidHead)
            }
        }
    }

    pub fn is_ignored(&self, path: &Path) -> Result<bool, Error> {
        Ok(self.git_repo.is_path_ignored(path)?)
    }
}

#[cfg(test)]
pub mod tests {
    use std::path::Path;

    use git2::Signature;
    use tempfile::{tempdir, NamedTempFile};

    use super::*;

    use crate::util::tests::*;

    const TEST_REMOTE_NAME: &'static str = "test";

    fn test_repo_with_files(path: &Path) -> (Repository, Config) {
        let (repo, config) = test_repo(path);
        NamedTempFile::new_in(path).unwrap().keep().unwrap();
        (repo, config)
    }

    fn test_repo_with_remote(path: &Path, remote_path: &Path) -> (Repository, Repository, Config) {
        let (repo, config) = test_repo_with_files(path);
        let remote_repo = Repository::init_bare(remote_path).unwrap();
        repo.remote(
            TEST_REMOTE_NAME,
            &format!("file://{}", remote_repo.path().to_str().unwrap()),
        )
        .unwrap();
        repo.config()
            .unwrap()
            .set_bool(&format!("remote.{}.snapshotenabled", "test"), true)
            .unwrap();
        (repo, remote_repo, config)
    }

    fn commit_all(repo: &Repository) {
        let mut index = Index::new().unwrap();
        repo.set_index(&mut index).unwrap();
        index
            .add_all(&["*"], IndexAddOption::DEFAULT, None)
            .unwrap();
        let tree = index.write_tree().unwrap();
        let tree = repo.find_tree(tree).unwrap();

        let signature = Signature::now("test", "test").unwrap();

        repo.commit(Some("HEAD"), &signature, &signature, "", &tree, &[])
            .unwrap();
    }

    pub fn check_snapshot_exists(repo: &Repo) -> bool {
        let config = repo.git_repo.config().unwrap();
        let snapshot_branch = Repo::snapshot_branch(&config, &repo.current_branch().unwrap());
        repo.git_repo
            .resolve_reference_from_short_name(&snapshot_branch)
            .is_ok()
    }

    #[test]
    fn snapshot() {
        let temp_dir = tempdir().unwrap();
        let (repo, _config) = test_repo_with_files(temp_dir.path());

        commit_all(&repo);

        NamedTempFile::new_in(temp_dir.path())
            .unwrap()
            .keep()
            .unwrap();

        let repo = Repo::new(repo);
        repo.snapshot().unwrap();
    }

    #[test]
    fn test_snapshot_empty_branch() {
        let temp_dir = tempdir().unwrap();
        let (repo, _config) = test_repo_with_files(temp_dir.path());

        let repo = Repo::new(repo);
        repo.snapshot().unwrap();

        assert!(check_snapshot_exists(&repo))
    }

    #[test]
    fn snapshot_no_changes() {
        let temp_dir = tempdir().unwrap();
        let (repo, config) = test_repo_with_files(temp_dir.path());

        commit_all(&repo);

        NamedTempFile::new_in(temp_dir.path())
            .unwrap()
            .keep()
            .unwrap();

        let repo = Repo::new(repo);
        repo.snapshot().unwrap();

        let current_branch = repo.current_branch().unwrap();
        let snapshot_branch = Repo::snapshot_branch(&config, &current_branch);
        let snapshot_ref = repo
            .git_repo
            .resolve_reference_from_short_name(&snapshot_branch)
            .unwrap();

        let first_commit = snapshot_ref.peel_to_commit().unwrap();

        repo.snapshot().unwrap();

        let second_commit = snapshot_ref.peel_to_commit().unwrap();

        assert_eq!(first_commit.id(), second_commit.id());
    }

    #[test]
    fn snapshot_branch_config_disabled() {
        let temp_dir = tempdir().unwrap();
        let (repo, config) = test_repo_with_files(temp_dir.path());

        let repo = Repo::new(repo);

        let current_branch = repo.current_branch().unwrap();
        let snapshot_branch = Repo::snapshot_branch(&config, &current_branch);
        repo.git_repo()
            .config()
            .unwrap()
            .set_bool(&format!("branch.{}.snapshotenabled", current_branch), false)
            .unwrap();

        repo.snapshot().unwrap();

        let ref_result = repo
            .git_repo
            .resolve_reference_from_short_name(&snapshot_branch);

        assert_eq!(ErrorCode::NotFound, ref_result.err().unwrap().code());
    }

    #[test]
    fn snapshot_branch_config_snapshotbranch() {
        let temp_dir = tempdir().unwrap();
        let (repo, _config) = test_repo_with_files(temp_dir.path());

        let repo = Repo::new(repo);

        let current_branch = repo.current_branch().unwrap();
        let snapshot_branch = "snapshottest";

        repo.git_repo()
            .config()
            .unwrap()
            .set_str(
                &format!("branch.{}.snapshotbranch", current_branch),
                snapshot_branch,
            )
            .unwrap();

        repo.snapshot().unwrap();

        assert!(check_snapshot_exists(&repo));
    }

    #[test]
    fn snapshot_snapshot_config_snapshotbranch() {
        let temp_dir = tempdir().unwrap();
        let (repo, _config) = test_repo_with_files(temp_dir.path());

        let repo = Repo::new(repo);

        let snapshot_branch = "snapshottest";

        repo.git_repo()
            .config()
            .unwrap()
            .set_str("snapshot.snapshotbranch", snapshot_branch)
            .unwrap();

        repo.snapshot().unwrap();

        assert!(check_snapshot_exists(&repo));
    }

    #[test]
    fn snapshot_snapshot_config_env_expansion() {
        let temp_dir = tempdir().unwrap();
        let (repo, _config) = test_repo_with_files(temp_dir.path());

        let repo = Repo::new(repo);
        std::env::set_var("TEST_NAME", "test");

        let snapshot_branch = "snapshottest/${TEST_NAME}";

        repo.git_repo()
            .config()
            .unwrap()
            .set_str("snapshot.snapshotbranch", snapshot_branch)
            .unwrap();

        repo.snapshot().unwrap();

        assert!(check_snapshot_exists(&repo));
    }

    #[test]
    fn snapshot_remote_push() {
        let temp_dir = tempdir().unwrap();
        let remote_dir = tempdir().unwrap();

        let (repo, remote_repo, config) = test_repo_with_remote(temp_dir.path(), remote_dir.path());

        let repo = Repo::new(repo);
        repo.snapshot().unwrap();

        let current_branch = repo.current_branch().unwrap();
        let snapshot_branch = Repo::snapshot_branch(&config, &current_branch);

        assert_eq!(
            None,
            remote_repo
                .resolve_reference_from_short_name(&snapshot_branch)
                .err()
        );
    }

    #[test]
    fn snapshot_remote_config_snapshotdisabled() {
        let temp_dir = tempdir().unwrap();
        let remote_dir = tempdir().unwrap();

        let (repo, remote_repo, mut config) =
            test_repo_with_remote(temp_dir.path(), remote_dir.path());

        config
            .set_bool(
                &format!("remote.{}.snapshotenabled", TEST_REMOTE_NAME),
                false,
            )
            .unwrap();

        let repo = Repo::new(repo);
        repo.snapshot().unwrap();

        let current_branch = repo.current_branch().unwrap();
        let snapshot_branch = Repo::snapshot_branch(&config, &current_branch);

        assert_eq!(
            ErrorCode::NotFound,
            remote_repo
                .resolve_reference_from_short_name(&snapshot_branch)
                .err()
                .unwrap()
                .code()
        );
    }

    #[test]
    fn snapshot_remote_config_snapshotbranch() {
        let temp_dir = tempdir().unwrap();
        let remote_dir = tempdir().unwrap();

        let (repo, remote_repo, mut config) =
            test_repo_with_remote(temp_dir.path(), remote_dir.path());

        let remote_branch = "snapshotremote/test";

        config
            .set_str(
                &format!("remote.{}.snapshotbranch", TEST_REMOTE_NAME),
                remote_branch,
            )
            .unwrap();

        let repo = Repo::new(repo);
        repo.snapshot().unwrap();

        assert_eq!(
            None,
            remote_repo
                .resolve_reference_from_short_name(remote_branch)
                .err()
        );
    }

    #[test]
    fn snapshot_invalid_head() {
        let temp_dir = tempdir().unwrap();

        let (repo, _config) = test_repo_with_files(temp_dir.path());

        commit_all(&repo);

        repo.set_head_detached(repo.head().unwrap().peel_to_commit().unwrap().id())
            .unwrap();

        let repo = Repo::new(repo);

        assert!(matches!(repo.snapshot().err().unwrap(), Error::InvalidHead));
    }

    #[test]
    fn repo_from_path() {
        let temp_dir = tempdir().unwrap();

        let (repo, _config) = test_repo(temp_dir.path());

        commit_all(&repo);

        repo.set_head_detached(repo.head().unwrap().peel_to_commit().unwrap().id())
            .unwrap();

        assert!(Repo::from_path(temp_dir.path()).is_ok());
    }
}
