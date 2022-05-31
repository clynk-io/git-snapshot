use std::env::var;

use git2::Config;
use shellexpand::env_with_context_no_errors;

pub const BRANCH_REF_PREFIX: &'static str = "refs/heads/";

fn get_value<T>(
    config: &Config,
    getter: &mut impl FnMut(&Config, &str) -> Result<T, git2::Error>,
    keys: &[&str],
    default_value: T,
) -> T {
    for &key in keys {
        if let Ok(value) = getter(config, key) {
            return value;
        }
    }
    return default_value;
}

// trait to easily find the first populated key in git config
pub trait ConfigValue {
    fn from_config(config: &Config, keys: &[&str], default_value: Self) -> Self
    where
        Self: Sized;
}

impl ConfigValue for String {
    fn from_config(config: &Config, keys: &[&str], default_value: Self) -> Self
    where
        Self: Sized,
    {
        get_value(config, &mut Config::get_string, keys, default_value)
    }
}

impl ConfigValue for bool {
    fn from_config(config: &Config, keys: &[&str], default_value: Self) -> Self
    where
        Self: Sized,
    {
        get_value(config, &mut Config::get_bool, keys, default_value)
    }
}

pub fn expand(input: &str, context: &[(&str, &str)]) -> String {
    env_with_context_no_errors(input, |name| {
        for &(key, val) in context {
            if name == key {
                return Some(val.to_owned());
            }
        }
        var(name).ok()
    })
    .to_string()
}

pub fn branch_ref_shorthand(ref_name: &str) -> &str {
    ref_name.trim_start_matches(BRANCH_REF_PREFIX)
}

#[cfg(test)]
pub mod tests {
    use std::path::Path;

    use super::*;
    use git2::Repository;
    use tempfile::tempdir;

    pub fn test_repo(path: &Path) -> (Repository, Config) {
        let repo = Repository::init(path).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@test.test").unwrap();

        (repo, config)
    }

    #[test]
    fn string_from_config() {
        let temp = tempdir().unwrap();

        let (_repo, mut config) = test_repo(temp.path());
        let key = "test.key";
        let value = "test_value";
        config.set_str(key, value).unwrap();

        let result = String::from_config(&config, &[key], String::new());
        assert_eq!(value, result);
    }

    #[test]
    fn bool_from_config() {
        let temp = tempdir().unwrap();

        let (_repo, mut config) = test_repo(temp.path());
        let key = "test.key";
        let value = true;
        config.set_bool(key, value).unwrap();

        let result = bool::from_config(&config, &[key], false);
        assert_eq!(value, result);
    }

    #[test]
    fn default_value() {
        let temp = tempdir().unwrap();

        let (_repo, config) = test_repo(temp.path());
        let key = "test.key";
        let default_value = "default_value";

        let result = String::from_config(&config, &[key], default_value.to_owned());
        assert_eq!(default_value, result);
    }

    #[test]
    fn mutliple_keys() {
        let temp = tempdir().unwrap();

        let (_repo, mut config) = test_repo(temp.path());
        let key1 = "test.key1";
        let key2 = "test.key2";

        let value = "test_value";
        config.set_str(key2, value).unwrap();

        let result = String::from_config(&config, &[key1, key2], String::new());
        assert_eq!(value, result);
    }
}
