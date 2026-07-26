//! Serde-serialisable metadata types that flow through the bun2nix pipeline:
//!
//! * [`PackageMeta`] — the registry view of a package (name + all versions).
//! * [`VersionMeta`] — per-version fields extracted from the npm registry manifest.
//! * [`EntryMeta`] — the per-entry record written into `bun.nix` and consumed
//!   by the manifest tool (Task 7) to assemble offline `.npm` cache files.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The subset of an npm registry document needed to build a bun `.npm` manifest
/// cache entry. Contains one or more versions of a single package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMeta {
    /// Bare npm package name, e.g. `"@neoconfetti/svelte"` or `"ms"`.
    pub name: String,
    /// At least one version; caller must sort or the builder will sort for you.
    pub versions: Vec<VersionMeta>,
}

/// Per-version metadata extracted from the npm registry `vnd.npm.install-v1`
/// abbreviated manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMeta {
    /// Semver version string, e.g. `"2.2.2"`.
    pub version: String,
    /// Full tarball URL, e.g.
    /// `"https://registry.npmjs.org/@neoconfetti/svelte/-/svelte-2.2.2.tgz"`.
    pub tarball_url: String,
    /// SRI integrity string, e.g. `"sha512-<base64>"`.  The npm client (Task 5)
    /// may leave this empty; the manifest tool (Task 7) fills it from
    /// [`EntryMeta::hash`] before calling the builder.
    pub integrity: String,
    /// `"dependencies"` map from the registry manifest.
    pub dependencies: BTreeMap<String, String>,
    /// `"peerDependencies"` map.
    pub peer_dependencies: BTreeMap<String, String>,
    /// `"optionalDependencies"` map.
    pub optional_dependencies: BTreeMap<String, String>,
    /// Names of peer dependencies that are marked optional in
    /// `"peerDependenciesMeta"`.
    pub optional_peers: Vec<String>,
    /// `"bin"` map (command → relative path).
    pub bin: BTreeMap<String, String>,
    /// `"os"` list from the registry manifest (`[]` → all OSes).
    pub os: Vec<String>,
    /// `"cpu"` list from the registry manifest (`[]` → all architectures).
    pub cpu: Vec<String>,
    /// `true` when the package has a non-empty `install` / `preinstall` /
    /// `postinstall` script, or when the registry sets `"hasInstallScript"`.
    pub has_install_script: bool,
}

/// The per-`bun.nix`-entry record materialised by the Nix layer (Task 8) and
/// consumed by the manifest tool (Task 7) to write `.npm` cache files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryMeta {
    /// `"<name>@<version>"`, the unique key used to look up this entry.
    pub name_version: String,
    /// Nix store hash of the fetched tarball (used as the SRI `sha512` value
    /// after base64-encoding when `manifest.integrity` is empty).
    pub hash: String,
    /// Registry base URL (without trailing slash). `None` → use the default
    /// `https://registry.npmjs.org` registry.
    pub registry: Option<String>,
    /// Full version metadata for this entry.
    pub manifest: VersionMeta,
}
