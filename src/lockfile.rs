use std::{collections::HashMap, str::FromStr};

use futures::{stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    error::{Error, Result},
    PrefetchedPackage,
};

const CONCURRENT_FETCH_REQUESTS: usize = 100;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// # Bun Lockfile
///
/// A model of the fields that exist in a bun lockfile in order to serve as a deserialization
/// target
pub struct Lockfile {
    /// The version field of the bun lockfile
    pub lockfile_version: u8,

    /// The workspaces declaration in the bun lockfile
    #[serde(default)]
    pub workspaces: HashMap<String, Workspace>,

    /// The list of all packages needed by the lockfile
    #[serde(default)]
    pub packages: HashMap<String, Package>,
}

impl Lockfile {
    fn parse_to_value(lockfile: &str) -> Result<Value> {
        jsonc_parser::parse_to_serde_value(lockfile, &Default::default())?
            .ok_or(Error::NoJsoncValue)
    }

    /// Use the lockfile's packages to produce prefetched sha256s for each
    pub async fn prefetch_packages(self) -> Result<Vec<PrefetchedPackage>> {
        stream::iter(self.packages)
            .map(|(_, package)| async move {
                let url = package.to_npm_url()?;

                PrefetchedPackage::prefetch(package.0, url, package.2.bin).await
            })
            .buffer_unordered(CONCURRENT_FETCH_REQUESTS)
            .try_collect()
            .await
    }
}

impl FromStr for Lockfile {
    type Err = Error;

    fn from_str(lockfile: &str) -> std::result::Result<Self, Self::Err> {
        let value = Self::parse_to_value(lockfile)?;

        Ok(serde_json::from_value(value)?)
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Workspace {
    /// The name of the workspace
    pub name: Option<String>,
    dependencies: HashMap<String, String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Package(pub String, String, MetaData, String);

impl Package {
    /// # NPM url converter
    ///
    /// Takes a package in the form:
    /// ```jsonc
    /// ["@alloc/quick-lru@5.2.0", "", {}, ""]
    /// ```
    ///
    /// And builds a prefetchable npm url like:
    /// ```bash
    /// https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz
    /// ```
    pub fn to_npm_url(&self) -> Result<String> {
        let Some((user, name_and_ver)) = self.0.split_once("/") else {
            let Some((name, ver)) = self.0.split_once("@") else {
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
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MetaData {
    peer_dependencies: HashMap<String, String>,
    optional_peers: Vec<String>,
    bin: Binaries,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Binaries {
    #[default]
    None,
    Unnamed(String),
    Named(HashMap<String, String>),
}

#[test]
fn test_parse_to_value_with_sample() {
    let sample = r#"
        // Allow comments as per jsonc spec
        {
            "name": "John Doe",
            "age": 43,
        }"#;

    let value = Lockfile::parse_to_value(sample).unwrap();

    assert!(value["name"] == "John Doe");
    assert!(value["age"] == 43);
}

#[test]
fn test_parse_to_value_empty() {
    let sample = "";

    let value = Lockfile::parse_to_value(sample).unwrap_err();

    assert!(value.to_string() == "Failed to parse empty lockfile, make sure you are providing a file with text contents.");
}

#[test]
fn test_from_str_version_only() {
    let lockfile = r#"
        {
            "lockfileVersion": 1,
        }"#;

    let value: Lockfile = lockfile.parse().unwrap();

    assert!(value.lockfile_version == 1);
}

#[test]
fn test_to_npm_url() {
    let package = Package(
        "bun-types@1.2.4".to_owned(),
        "".to_owned(),
        MetaData::default(),
        "".to_owned(),
    );

    let out = package.to_npm_url().unwrap();

    assert!(out == "https://registry.npmjs.org/bun-types/-/bun-types-1.2.4.tgz");
}

#[test]
fn test_to_npm_url_with_namespace() {
    let package = Package(
        "@alloc/quick-lru@5.2.0".to_owned(),
        "".to_owned(),
        MetaData::default(),
        "".to_owned(),
    );

    let out = package.to_npm_url().unwrap();

    assert!(out == "https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz");
}
