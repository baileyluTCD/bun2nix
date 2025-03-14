use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    path::PathBuf,
    str::FromStr,
};

use futures::{stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{query_as, Connection, Executor, QueryBuilder, Sqlite, SqliteConnection};

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
    pub async fn prefetch_packages(
        self,
        cache_location: Option<PathBuf>,
    ) -> Result<Vec<PrefetchedPackage>> {
        let mut packages = self.packages.into_values().collect::<HashSet<_>>();

        let Some(loc) = cache_location else {
            return Self::fetch_uncached_packages(packages, None).await;
        };

        let mut cache = Self::connect_and_migrate(loc).await?;
        Self::create_temp_pkg_list_db(&packages, &mut cache).await?;

        let mut cached: Vec<PrefetchedPackage> = query_as(
            "SELECT p.name, p.url, p.hash, p.binaries
            FROM packages p
            INNER JOIN temp_packages t ON p.name = t.name",
        )
        .fetch_all(&mut cache)
        .await?;

        let uncached_names = query_as::<_, (String,)>(
            "SELECT DISTINCT t.name
             FROM temp_packages t
             LEFT JOIN packages p ON t.name = p.name
             WHERE p.name IS NULL",
        )
        .fetch_all(&mut cache)
        .await?
        .into_iter()
        .map(|x| x.0)
        .collect::<HashSet<_>>();

        packages.retain(|pkg| uncached_names.contains(&pkg.0));

        if packages.is_empty() {
            return Ok(cached);
        };

        let new_pkgs = Self::fetch_uncached_packages(packages, Some(cache)).await?;

        cached.extend(new_pkgs);

        Ok(cached)
    }

    async fn create_temp_pkg_list_db(
        packages: &HashSet<Package>,
        cache: &mut SqliteConnection,
    ) -> Result<()> {
        cache
            .execute("CREATE TEMP TABLE temp_packages (name TEXT NOT NULL PRIMARY KEY)")
            .await?;

        QueryBuilder::<Sqlite>::new("INSERT INTO temp_packages (name) ")
            .push_values(packages, |mut b, package| {
                b.push_bind(&package.0);
            })
            .build()
            .execute(cache)
            .await?;

        Ok(())
    }

    async fn connect_and_migrate(loc: PathBuf) -> Result<SqliteConnection> {
        let mut conn = SqliteConnection::connect(loc.to_str().unwrap_or_default()).await?;

        sqlx::migrate!().run(&mut conn).await?;

        Ok(conn)
    }

    async fn fetch_uncached_packages(
        packages: HashSet<Package>,
        cache: Option<SqliteConnection>,
    ) -> Result<Vec<PrefetchedPackage>> {
        let pkgs = stream::iter(packages)
            .map(|package| async {
                let url = package.to_npm_url()?;

                PrefetchedPackage::nix_store_fetch(package.0, url, package.2.bin).await
            })
            .buffer_unordered(CONCURRENT_FETCH_REQUESTS)
            .try_collect()
            .await?;

        let Some(mut cache) = cache else {
            return Ok(pkgs);
        };

        QueryBuilder::<Sqlite>::new("INSERT INTO packages (name, url, hash, binaries) ")
            .push_values(&pkgs, |mut b, pkg| {
                b.push_bind(&pkg.name);
                b.push_bind(&pkg.url);
                b.push_bind(&pkg.hash);
                b.push_bind(serde_json::to_string(&pkg.binaries).unwrap());
            })
            .build()
            .execute(&mut cache)
            .await?;

        Ok(pkgs)
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

impl Hash for Package {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Package {}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MetaData {
    peer_dependencies: HashMap<String, String>,
    optional_peers: Vec<String>,
    bin: Binaries,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Binaries {
    #[default]
    None,
    Unnamed(String),
    Named(HashMap<String, String>),
}

impl TryFrom<String> for Binaries {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        Ok(serde_json::from_str(&value)?)
    }
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
