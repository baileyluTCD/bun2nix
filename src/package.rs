//! This module holds the core implementation for the package type and related methods

use std::{
    fmt::Debug,
    hash::{Hash, Hasher},
};

use serde::{Deserialize, Serialize};
use state::State;

use crate::error::Result;

mod binaries;
mod identifier;
mod metadata;
mod normalized_binary;
mod state;

pub use binaries::Binaries;
pub use identifier::Identifier;
pub use metadata::MetaData;
pub use normalized_binary::NormalizedBinary;
pub use state::{Extracted, Normalized};

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
/// # Package
///
/// An individual package found in a bun lockfile.
pub struct Package<D: State> {
    /// The prefetched package hash
    pub hash: Option<String>,

    /// The name of the package, as found in the `./node_modules` directory or in an import
    /// statement
    pub name: String,

    /// The package's identifier string for fetching from npm
    pub identifier: Identifier,

    /// The state the package is currently in
    pub data: D,
}

impl Package<Extracted> {
    /// # Package Constructor
    ///
    /// Produce a new instance of a just extracted package
    pub fn new(
        name: String,
        identifier: Identifier,
        hash: Option<String>,
        binaries: Binaries,
    ) -> Self {
        Self {
            name,
            identifier,
            hash,
            data: Extracted { binaries },
        }
    }

    /// # Normalize Packages
    ///
    /// Normalizes a package's data fields to prepare it to be output
    ///
    /// This includes building the output path in `node_modules` and a proper binaries list
    pub fn normalize(self) -> Result<Package<Normalized>> {
        Ok(Package {
            data: Normalized {
                out_path: Normalized::convert_name_to_out_path(&self.name),
                url: self.identifier.to_url()?,
                binaries: self.data.binaries.normalize(&self.name),
            },
            identifier: self.identifier,
            hash: self.hash,
            name: self.name,
        })
    }
}

impl<D: State> Hash for Package<D> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.identifier.hash(state);
    }
}

impl<D: State> PartialEq for Package<D> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.identifier == other.identifier
    }
}

impl<D: State> PartialOrd for Package<D> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<D: State> Eq for Package<D> {}

impl<D: State> Ord for Package<D> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.name, &self.identifier).cmp(&(&other.name, &other.identifier))
    }
}
