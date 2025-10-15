//! This module holds the core implementation for the package type and related methods

use std::{
    fmt::Debug,
    hash::{Hash, Hasher},
};

use serde::{Deserialize, Serialize};

mod fetcher;

pub use fetcher::Fetcher;

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
/// # Package
///
/// An individual package found in a bun lockfile.
pub struct Package {
    /// The name of the package, as found in the `./node_modules` directory or in an import
    /// statement
    pub name: String,

    /// The fetch method to use for the package
    pub fetcher: Fetcher,
}

impl Package {
    pub fn from_raw_npm_identifier(name: String, fetcher: Fetcher) -> Self {
        let npm_identifier_file_safe = name.replace("/", "+");

        Self::from_file_safe_npm_identifier(npm_identifier_file_safe, fetcher)
    }

    pub fn from_file_safe_npm_identifier(name: String, fetcher: Fetcher) -> Self {
        assert!(
            !name.contains("/"),
            "File safe npm identifier cannot contain a `/` character, please use the from raw method instead"
        );

        Self { name, fetcher }
    }

    pub fn from_workspace_identifier(name: String, fetcher: Fetcher) -> Self {
        Self { name, fetcher }
    }
}

impl Hash for Package {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.fetcher.hash(state);
    }
}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.fetcher == other.fetcher
    }
}

impl PartialOrd for Package {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Package {}

impl Ord for Package {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.name, &self.fetcher).cmp(&(&other.name, &other.fetcher))
    }
}
