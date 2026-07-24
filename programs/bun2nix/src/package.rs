//! This module holds the core implementation for the package type and related methods

use std::{
    fmt::{Debug, Write as _},
    hash::{Hash, Hasher},
};

use bun2nix_core::manifest::meta::VersionMeta;
use serde::Serialize;

mod fetcher;
mod manifest_nix;

pub use fetcher::{DEFAULT_REGISTRY, Fetcher};

#[derive(Debug, Serialize, Clone)]
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

    /// Optional registry manifest metadata for this package.
    ///
    /// Only default-registry npm packages ever carry `Some`; git, GitHub,
    /// tarball, workspace and non-default-registry packages always stay `None`.
    /// When present, the rendered `bun.nix` entry gains a `manifest = { ... }`
    /// attribute and its `url` is taken from the manifest's `tarball_url`.
    #[serde(skip)]
    pub manifest: Option<VersionMeta>,
}

impl Package {
    /// # Create New Package
    ///
    /// Creates a given package using it's name
    /// and fetcher information
    pub fn new(name: String, fetcher: Fetcher) -> Self {
        Self {
            name,
            fetcher,
            manifest: None,
        }
    }

    /// # Attach Manifest
    ///
    /// Attaches registry manifest metadata to this package. When the fetcher is
    /// an npm `FetchUrl`, its `url` is overwritten with the manifest's
    /// authoritative `tarball_url` (the `hash` is left untouched). For any other
    /// fetcher the url is left as-is.
    pub fn with_manifest(mut self, manifest: VersionMeta) -> Self {
        if let Fetcher::FetchUrl { url, .. } = &mut self.fetcher {
            url.clone_from(&manifest.tarball_url);
        }

        self.manifest = Some(manifest);
        self
    }

    /// # Manifest Nix Suffix
    ///
    /// Renders the trailing `// { manifest = { ... }; }` attrset-union suffix
    /// for this package's `bun.nix` entry, or an empty string when there is no
    /// manifest. Invoked directly from the output template.
    ///
    /// The output is deterministic (all maps are `BTreeMap`s) and every string
    /// value/key is escaped for Nix. No `integrity` key is emitted — it is
    /// reconstructed downstream from the entry `hash`.
    pub fn manifest_nix(&self) -> String {
        match &self.manifest {
            None => String::new(),
            Some(meta) => {
                let mut out = String::new();
                // Indent so the block aligns under the entry (entry is at 2
                // spaces, the fetcher's closing brace at 2 spaces).
                let _ = write!(out, " // {{\n    manifest = ");
                manifest_nix::render_version_meta(&mut out, meta, 4);
                out.push_str(";\n  }");
                out
            }
        }
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
