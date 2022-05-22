use crate::error::Error;
use git::refs::FullNameRef;
use git2::{Index, IndexAddOption, Repository, Signature};
use git_repository as git;
use std::path::Path;

pub struct RepoConfig {
    path: String,
    snapshot_branch: String,
    remotes: Vec<Remote>,
    frequency: u64,
}

pub struct Repo {
    git_repo: Repository,
}

pub struct Remote {
    name: String,
    snapshot_branch: String,
}

impl Repo {
    pub fn new(path: impl AsRef<Path>, snapshot_branch: Option<String>) -> Result<Self, Error> {
        let git_repo = Repository::discover(path)?;
        println!("{:?}", git_repo.find_reference("HEAD").unwrap().target());
        Ok(Self { git_repo })
    }

    pub fn snapshot(&self) -> Result<(), Error> {
        let current_branch = self.current_branch()?;
        print!("Branch: {}", current_branch);
        let ref_name = format!("refs/heads/snapshots/{}", current_branch);
        let mut index = Index::new()?;
        self.git_repo.set_index(&mut index)?;
        index.add_all(&["*"], IndexAddOption::DEFAULT, None)?;
        let tree = index.write_tree()?;
        let tree = self.git_repo.find_tree(tree)?;

        let signature = Signature::now("asdf", "asdf@asdg.com")?;

        let parent = self
            .git_repo
            .find_reference(&ref_name)
            .and_then(|r| r.peel_to_commit())
            .ok();

        self.git_repo.commit(
            Some(&ref_name),
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
