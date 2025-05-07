use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Identifier {
    Npm(String),
    Workspace(String),
    Git(String),
    Tarball(String),
}

impl Identifier {
    /// # NPM url converter
    ///
    /// Produce a url needed to fetch from the npm api from a package
    ///
    /// ## Usage
    ///```rust
    /// use bun2nix::Identifer;
    ///
    /// let identifier = "@alloc/quick-lru@5.2.0";
    ///
    /// assert_eq!(Identifer::to_npm_url(identifier).unwrap(), "https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz")
    /// ```
    pub fn to_npm_url(npm_identifier: &str) -> Result<String> {
        let Some((user, name_and_ver)) = npm_identifier.split_once("/") else {
            let Some((name, ver)) = npm_identifier.split_once("@") else {
                return Err(Error::NoAtInPackageIdentifier);
            };

            return Ok(format!(
                "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                name, name, ver
            ));
        };

        let Some((name, ver)) = name_and_ver.split_once("@") else {
            return Err(Error::NoAtInPackageIdentifier);
        };

        Ok(format!(
            "https://registry.npmjs.org/{}/{}/-/{}-{}.tgz",
            user, name, name, ver
        ))
    }

    /// # Http url converter
    ///
    /// Produce a url needed to fetch a tarball or from git for a package
    ///
    /// ## Usage
    ///```rust
    /// use bun2nix::Identifer;
    ///
    /// let github_identifier = "lodash@github:lodash/lodash#8a26eb4",
    /// let gitssh_identifier = "is-even-min@git+ssh://gitlab.com/iamashley0/is-even-min.git#0af22132d7abba2b7c4bb94f1887ca30b1b102aa",
    ///
    /// assert_eq!(Identifer::to_git_url(github_identifier).unwrap(),
    /// "https://github.com/lodash/lodash")
    /// assert_eq!(Identifer::to_git_url(gitssh_identifier).unwrap(), "https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz")
    /// ```
    pub fn to_http_url(identifier: &str) -> Result<String> {
        let Some((_, url)) = identifier.split_once("@") else {
            return Err(Error::NoAtInPackageIdentifier);
        };

        return Ok(url.to_string());
    }

    pub fn to_url(&self) -> Result<String> {
        match &self {
            Self::Npm(npm_identifier) => Self::to_npm_url(npm_identifier),
            Self::Workspace(identifier) | Self::Tarball(identifier) | Self::Git(identifier) => {
                Self::to_http_url(identifier)
            }
        }
    }
}

impl Default for Identifier {
    fn default() -> Self {
        Self::Npm(String::default())
    }
}
