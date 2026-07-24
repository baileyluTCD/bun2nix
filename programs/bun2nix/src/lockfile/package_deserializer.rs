use std::collections::BTreeMap;

use bun2nix_core::manifest::meta::VersionMeta;

use crate::{
    Package,
    error::{Error, Result},
    package::Fetcher,
};

mod prefetch;
pub use prefetch::Prefetch;

/// The dependency-graph metadata bun stores inline as element 2 of an npm
/// lockfile entry (`[id, url, meta, hash]`). Mirrors the per-version fields of
/// the abbreviated npm registry manifest, which is why it reconstructs the same
/// `VersionMeta` the network path used to fetch.
#[derive(serde::Deserialize)]
struct RawLockMeta {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(rename = "optionalDependencies", default)]
    optional_dependencies: BTreeMap<String, String>,
    #[serde(rename = "peerDependencies", default)]
    peer_dependencies: BTreeMap<String, String>,
    /// Already split out by bun (no `peerDependenciesMeta` reconstruction needed).
    #[serde(rename = "optionalPeers", default)]
    optional_peers: Vec<String>,
    #[serde(default)]
    bin: RawBin,
    #[serde(default)]
    os: RawStringList,
    #[serde(default)]
    cpu: RawStringList,
}

/// npm `bin` can be a bare string or a `{ name: path }` object.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum RawBin {
    Str(String),
    Map(BTreeMap<String, String>),
}

impl Default for RawBin {
    fn default() -> Self {
        RawBin::Map(BTreeMap::new())
    }
}

/// `os`/`cpu` in bun.lock metadata can be a bare string (bun collapses
/// single-element lists, e.g. `"cpu": "x64"` or `"os": "none"`) or an array.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum RawStringList {
    Str(String),
    List(Vec<String>),
}

impl Default for RawStringList {
    fn default() -> Self {
        RawStringList::List(Vec::new())
    }
}

impl From<RawStringList> for Vec<String> {
    fn from(v: RawStringList) -> Self {
        match v {
            RawStringList::Str(s) => vec![s],
            RawStringList::List(l) => l,
        }
    }
}

impl RawLockMeta {
    /// Build the registry-shaped `VersionMeta` for `<name>@<version>` using the
    /// already-derived `tarball_url`. `integrity` is left empty (the Nix layer
    /// fills it from the entry hash) and `has_install_script` is `false` (the
    /// lockfile carries no install-script signal).
    fn into_version_meta(self, ident: &str, tarball_url: &str) -> Result<VersionMeta> {
        let (name, version) = ident
            .rsplit_once('@')
            .ok_or(Error::NoAtInPackageIdentifier)?;

        // For a bare-string bin, npm/bun key it by the package-name basename
        // (the last path segment, dropping any `@scope/`).
        let basename = name.rsplit('/').next().unwrap_or(name);
        let bin = match self.bin {
            RawBin::Str(path) => [(basename.to_string(), path)].into_iter().collect(),
            RawBin::Map(map) => map,
        };

        Ok(VersionMeta {
            version: version.to_string(),
            tarball_url: tarball_url.to_string(),
            integrity: String::new(),
            dependencies: self.dependencies,
            peer_dependencies: self.peer_dependencies,
            optional_dependencies: self.optional_dependencies,
            optional_peers: self.optional_peers,
            bin,
            os: self.os.into(),
            cpu: self.cpu.into(),
            has_install_script: false,
        })
    }
}

type Values = Vec<serde_json::Value>;

/// # Package Deserializer
///
/// Deserializes a given bun lockfile entry line into it's
/// name and nix fetcher implementation
#[derive(Debug)]
pub struct PackageDeserializer {
    /// The name for the package
    pub name: String,

    /// The list of serde json values for the tuple in question
    pub values: Values,
}

impl PackageDeserializer {
    /// # Deserialize package
    ///
    /// Deserialize a given package from it's lockfile representation
    ///
    /// Entries are dispatched on the identifier's resolution (the part after
    /// the package name), not on tuple arity: bun emits github entries with
    /// an integrity hash (arity 4) and remote/vendored tarball entries with
    /// inline metadata (arity 3), so arity alone cannot tell entry kinds
    /// apart.
    pub fn deserialize_package(name: String, values: Values) -> Result<Package> {
        let arity = values.len();
        let deserializer = Self { name, values };

        if arity == 1 {
            return deserializer.deserialize_workspace_package();
        }
        if !(2..=4).contains(&arity) {
            return Err(Error::UnexpectedPackageEntryLength(arity));
        }

        let resolution = deserializer
            .values
            .first()
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .and_then(drain_package_specifier)
            .ok_or(Error::NoAtInPackageIdentifier)?;

        if resolution.starts_with("github:") {
            Self::deserialize_github_package(resolution)
        } else if resolution.starts_with("git+") {
            Self::deserialize_git_package(resolution)
        } else if resolution.starts_with("http://") || resolution.starts_with("https://") {
            Self::deserialize_tarball_package(resolution)
        } else if arity == 4 {
            deserializer.deserialize_npm_package()
        } else {
            Self::deserialize_file_package(deserializer.name, resolution)
        }
    }

    /// # Deserialize an NPM Package
    ///
    /// Deserialize an npm package from it's bun lockfile representation
    ///
    /// This is found in the source as a tuple of arity 4:
    /// `[identifier, tarball_url, metadata, hash]`
    ///
    /// The tarball_url field is empty for the default registry (registry.npmjs.org),
    /// or contains the exact URL to the package tarball for non-default registries.
    pub fn deserialize_npm_package(mut self) -> Result<Package> {
        // The bun.lock format for npm packages is:
        // [identifier, tarball_url, metadata, hash]
        // - identifier: "name@version"
        // - tarball_url: "" for default registry, or exact URL to tarball
        // - metadata: object with dependencies, peerDependencies, bin, etc.
        // - hash: integrity hash (sha512-...)

        let npm_identifier_raw = swap_remove_value(&mut self.values, 0);
        // After swap_remove(0): [hash, tarball_url, meta]

        let hash = swap_remove_value(&mut self.values, 0);
        // After swap_remove(0): [meta, tarball_url]

        let tarball_url = self
            .values
            .get(1)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        debug_assert!(
            hash.contains("sha512-"),
            "Expected hash to be in sri format and contain sha512"
        );

        let fetcher = Fetcher::new_npm_package(&npm_identifier_raw, hash, tarball_url)?;

        // Every npm entry carries an offline manifest reconstructed from the
        // inline metadata object (index 0 after the swaps: `[meta, tarball_url]`).
        // The tarball URL is the fetcher's: inferred for the default registry,
        // verbatim from the lockfile otherwise.
        let manifest = if let Fetcher::FetchUrl { url, .. } = &fetcher {
            let raw: RawLockMeta = serde_json::from_value(self.values.swap_remove(0))?;
            Some(raw.into_version_meta(&npm_identifier_raw, url)?)
        } else {
            None
        };

        let package = Package::new(npm_identifier_raw, fetcher);
        Ok(match manifest {
            Some(m) => package.with_manifest(m),
            None => package,
        })
    }

    /// # Deserialize a Github Package
    ///
    /// Deserialize a github package from its `github:owner/repo#rev`
    /// resolution
    pub fn deserialize_github_package(id: String) -> Result<Package> {
        let (url, rev) = split_once_owned(id, '#').ok_or(Error::MissingGitRef)?;

        let prefetch_url = format!("{}?ref={}", &url, &rev);
        let prefetch = Prefetch::prefetch_package(&prefetch_url)?;

        let (owner_with_pre, repo) = split_once_owned(url, '/').ok_or(Error::ImproperGithubUrl)?;
        let owner = drop_prefix(owner_with_pre, "github:");

        let id_with_ver = format!("github:{}-{}-{}", &owner, &repo, &rev);

        let fetcher = Fetcher::FetchGitHub {
            owner,
            repo,
            rev,
            hash: prefetch.hash,
        };

        Ok(Package::new(id_with_ver, fetcher))
    }

    /// # Deserialize a Git Package
    ///
    /// Deserialize a git package from its `git+<url>#<rev>` resolution
    pub fn deserialize_git_package(id: String) -> Result<Package> {
        let git_url = drop_prefix(id, "git+");
        let (url, rev) = split_once_owned(git_url, '#').ok_or(Error::MissingGitRef)?;

        let prefetch_url = format!("git+{}?rev={}", &url, &rev);
        let prefetch = Prefetch::prefetch_package(&prefetch_url)?;

        let id_with_rev = format!("git:{}", &rev);

        let fetcher = Fetcher::FetchGit {
            url,
            rev,
            hash: prefetch.hash,
        };

        Ok(Package::new(id_with_rev, fetcher))
    }

    /// # Deserialize a file package
    ///
    /// Deserialize a file package from it's bun lockfile representation
    ///
    /// This is found in the source as a tuple of arity 2
    ///
    /// Handles both explicit `file:` prefix and inferred local paths.
    /// Bun strips the `file:` prefix for local tarballs in the packages section,
    /// so we need to infer local paths from `./` prefixes.
    ///
    /// See:
    /// - https://github.com/oven-sh/bun/blob/7ebfdf97a872908aeacce7af7eba21658b265ad7/src/install/dependency.zig#L514-L517
    /// - https://github.com/oven-sh/bun/blob/7ebfdf97a872908aeacce7af7eba21658b265ad7/src/install/resolution.zig#L46-L59
    pub fn deserialize_file_package(name: String, path: String) -> Result<Package> {
        debug_assert!(
            !path.contains("http"),
            "File path can never contain http, because then it would be a tarball"
        );

        // Strip prefix: explicit "file:" or implicit "./" (Bun strips file: for
        // local tarballs).  Vendored tarballs appear as bare relative paths
        // (e.g. "vendor/pkg-1.0.0.tgz") with no prefix at all.
        let path = path
            .strip_prefix("file:")
            .or_else(|| path.strip_prefix("./"))
            .unwrap_or(&path);

        Ok(Package::new(
            name,
            Fetcher::CopyToStore {
                path: path.to_string(),
            },
        ))
    }

    /// # Deserialize a tarball package
    ///
    /// Deserialize a tarball package from it's bun lockfile representation
    ///
    /// This is found in the source as a tuple of arity 2
    pub fn deserialize_tarball_package(url: String) -> Result<Package> {
        debug_assert!(url.contains("http"), "Expected tarball url to contain http");

        let prefetch = Prefetch::prefetch_package(&url)?;

        let name = format!("tarball:{}", url);
        let fetcher = Fetcher::FetchTarball {
            url,
            hash: prefetch.hash,
        };

        Ok(Package::new(name, fetcher))
    }

    /// # Deserialize a workspace package
    ///
    /// Deserialize a workspace package from it's bun lockfile representation
    ///
    /// This is found in the source as a tuple of arity 2
    pub fn deserialize_workspace_package(mut self) -> Result<Package> {
        let id = swap_remove_value(&mut self.values, 0);
        let path = Self::drain_after_substring(id, "workspace:")
            .ok_or(Error::MissingWorkspaceSpecifier)?;

        Ok(Package::new(self.name, Fetcher::CopyToStore { path }))
    }

    fn drain_after_substring(mut input: String, sub: &str) -> Option<String> {
        let pos = input.rfind(sub)? + sub.len();

        Some(input.drain(pos..).collect())
    }
}

/// # Swap Remove `Value`
///
/// Remove a value from a serde_json `Values` array, and take ownership
/// of it in a fast way by swapping in the final value of the array.
///
///```rust
/// use bun2nix::lockfile::swap_remove_value;
/// use serde_json::json;
///
/// let mut values = vec![
///  json!("@types/bun@1.2.4"),
///  json!({}),
///  json!([]),
///  json!("sha512-QtuV5OMR8/rdKJs213iwXDpfVvnskPXY/S0ZiFbsTjQZycuqPbMW8Gf/XhLfwE5njW8sxI2WjISURXPlHypMFA==")
/// ];
///
/// assert_eq!(
///     swap_remove_value(&mut values, 0),
///     "@types/bun@1.2.4"
/// );
/// assert_eq!(
///     swap_remove_value(&mut values, 0),
///     "sha512-QtuV5OMR8/rdKJs213iwXDpfVvnskPXY/S0ZiFbsTjQZycuqPbMW8Gf/XhLfwE5njW8sxI2WjISURXPlHypMFA=="
/// );
/// ```
pub fn swap_remove_value(values: &mut Values, index: usize) -> String {
    let mut value = values.swap_remove(index).to_string();

    debug_assert!(value.starts_with('"'), "Value should start with a quote");
    debug_assert!(value.ends_with('"'), "Value should end with a quote");

    value.drain(1..value.len() - 1).collect()
}

/// # Drain Package Specifier
///
/// Consumes a bun package identifier of the form `<name>@<specifier>` and
/// returns the owned `<specifier>` (a version, tarball url or local path).
///
/// A package name may be scoped (e.g. `@solidjs/start`), so the leading `@` is
/// part of the name rather than the separator. The specifier may also contain
/// `@` characters, such as the tarball url `https://pkg.pr.new/@scope/pkg@rev`,
/// hence we split on the first `@` after any leading scope rather than the last.
///
///```rust
/// use bun2nix::lockfile::drain_package_specifier;
///
/// assert_eq!(
///     drain_package_specifier("zod@https://registry.npmjs.org/zod/-/zod-3.21.4.tgz".to_owned()),
///     Some("https://registry.npmjs.org/zod/-/zod-3.21.4.tgz".to_owned())
/// );
///
/// assert_eq!(
///     drain_package_specifier("@solidjs/start@https://pkg.pr.new/@solidjs/start@dfb2020".to_owned()),
///     Some("https://pkg.pr.new/@solidjs/start@dfb2020".to_owned())
/// );
///
/// assert_eq!(
///     drain_package_specifier("@my/pkg@git+ssh://git@github.com/my/pkg.git#abc123".to_owned()),
///     Some("git+ssh://git@github.com/my/pkg.git#abc123".to_owned())
/// );
///
/// assert_eq!(
///     drain_package_specifier("no-at-here".to_owned()),
///     None
/// );
/// ```
pub fn drain_package_specifier(mut id: String) -> Option<String> {
    let search_start = usize::from(id.starts_with('@'));
    let sep = id[search_start..].find('@')? + search_start;

    Some(id.drain(sep + 1..).collect())
}

/// # Split Once (Owned)
///
/// Variant of `String::split_once` which consumes the original string and produces
/// two owned values as an output (without a new allocation).
///
///```rust
/// use bun2nix::lockfile::split_once_owned;
///
/// let input = "hello#world".to_owned();
///
/// assert_eq!(
///     split_once_owned(input, '#'),
///     Some(("hello".to_owned(), "world".to_owned()))
/// );
/// ```
pub fn split_once_owned(mut input: String, char: char) -> Option<(String, String)> {
    let split_pos = input.find(char)?;

    let mut first: String = input.drain(..=split_pos).collect();
    first.pop();

    Some((first, input))
}

/// # Drop Prefix
///
/// Consumes an owned string with a known prefix and returns an owned
/// value without that prefix (reuses the old allocation).
///
///```rust
/// use bun2nix::lockfile::drop_prefix;
///
/// let input = "hello:world".to_owned();
///
/// assert_eq!(
///     drop_prefix(input, "hello:"),
///     "world"
/// );
/// ```
pub fn drop_prefix(mut input: String, prefix: &str) -> String {
    if input.starts_with(prefix) {
        input.drain(..prefix.len());
    }

    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "sha512-t0BRVXvbiE/o20Hfw669rLbMCDWtYZLvmJigy2f0MxsXF+71pxhR3xOkspmsO8h3ZlNzyibAmtCa3l4lYKk6gQ==";

    // A plain npm entry (arity 4, bare version resolution) still routes to the
    // npm deserializer.
    #[test]
    fn npm_entry_dispatches_to_npm_package() {
        let values = vec![
            json!("react-dom@19.2.7"),
            json!(""),
            json!({ "dependencies": { "scheduler": "^0.27.0" } }),
            json!(SHA),
        ];

        let pkg = PackageDeserializer::deserialize_package("react-dom".into(), values).unwrap();
        assert!(
            matches!(pkg.fetcher, Fetcher::FetchUrl { ref url, .. }
                if url == "https://registry.npmjs.org/react-dom/-/react-dom-19.2.7.tgz"),
            "expected FetchUrl, got {:?}",
            pkg.fetcher
        );
    }

    // Vendored tarballs are arity-3 entries whose resolution is a bare
    // relative path: [id, meta, integrity].
    #[test]
    fn vendored_tarball_dispatches_to_file_package() {
        let values = vec![
            json!("@opencode-ai/client@vendor/opencode-ai-client-1.17.13.tgz"),
            json!({}),
            json!(SHA),
        ];

        let pkg = PackageDeserializer::deserialize_package(
            "@opencode-ai/app/@opencode-ai/client".into(),
            values,
        )
        .unwrap();

        assert!(
            matches!(pkg.fetcher, Fetcher::CopyToStore { ref path }
                if path == "vendor/opencode-ai-client-1.17.13.tgz"),
            "expected CopyToStore, got {:?}",
            pkg.fetcher
        );
    }

    // file: and ./ prefixes are still stripped from file-package paths.
    #[test]
    fn prefixed_file_paths_are_stripped() {
        for id in ["local-pkg@file:local/pkg.tgz", "local-pkg@./local/pkg.tgz"] {
            let values = vec![json!(id), json!(SHA)];
            let pkg = PackageDeserializer::deserialize_package("local-pkg".into(), values).unwrap();
            assert!(
                matches!(pkg.fetcher, Fetcher::CopyToStore { ref path } if path == "local/pkg.tgz"),
                "expected stripped CopyToStore for {id}, got {:?}",
                pkg.fetcher
            );
        }
    }

    #[test]
    fn npm_entry_reconstructs_manifest_from_lockfile_meta() {
        let values = vec![
            json!("react-dom@19.2.7"),
            json!(""),
            json!({
                "dependencies": { "scheduler": "^0.27.0" },
                "peerDependencies": { "react": "^19.2.7" }
            }),
            json!(SHA),
        ];

        let pkg = PackageDeserializer::deserialize_package("react-dom".into(), values).unwrap();
        let m = pkg
            .manifest
            .expect("a default-registry npm entry must carry a manifest");

        assert_eq!(m.version, "19.2.7");
        assert_eq!(
            m.tarball_url,
            "https://registry.npmjs.org/react-dom/-/react-dom-19.2.7.tgz"
        );
        assert!(
            m.integrity.is_empty(),
            "integrity is reused from the entry hash"
        );
        assert_eq!(
            m.dependencies.get("scheduler"),
            Some(&"^0.27.0".to_string())
        );
        assert_eq!(
            m.peer_dependencies.get("react"),
            Some(&"^19.2.7".to_string())
        );
        assert!(m.optional_dependencies.is_empty());
        assert!(m.optional_peers.is_empty());
        assert!(m.bin.is_empty());
        assert!(m.os.is_empty());
        assert!(m.cpu.is_empty());
        assert!(!m.has_install_script);
    }

    #[test]
    fn optional_peers_and_object_bin_pass_through() {
        let values = vec![
            json!("next@16.0.3"),
            json!(""),
            json!({
                "peerDependencies": { "react": "^19.0.0", "sass": "^1.3.0" },
                "optionalPeers": ["sass"],
                "bin": { "next": "dist/bin/next" }
            }),
            json!(SHA),
        ];

        let pkg = PackageDeserializer::deserialize_package("next".into(), values).unwrap();
        let m = pkg.manifest.unwrap();

        assert_eq!(m.optional_peers, vec!["sass".to_string()]);
        assert_eq!(m.bin.get("next"), Some(&"dist/bin/next".to_string()));
    }

    // bun collapses single-element os/cpu lists to bare strings in bun.lock
    // (e.g. platform packages like @cloudflare/workerd-darwin-64).
    #[test]
    fn string_form_os_cpu_parse_as_single_element_lists() {
        let values = vec![
            json!("@cloudflare/workerd-darwin-64@1.20251118.0"),
            json!(""),
            json!({ "os": "darwin", "cpu": "x64" }),
            json!(SHA),
        ];

        let pkg = PackageDeserializer::deserialize_package(
            "@cloudflare/workerd-darwin-64".into(),
            values,
        )
        .unwrap();
        let m = pkg.manifest.unwrap();

        assert_eq!(m.os, vec!["darwin".to_string()]);
        assert_eq!(m.cpu, vec!["x64".to_string()]);
    }

    #[test]
    fn bare_string_bin_normalizes_to_scoped_basename() {
        let values = vec![
            json!("@neoconfetti/svelte@2.2.2"),
            json!(""),
            json!({ "bin": "./cli.js" }),
            json!(SHA),
        ];

        let pkg =
            PackageDeserializer::deserialize_package("@neoconfetti/svelte".into(), values).unwrap();
        let m = pkg.manifest.unwrap();

        // Bare-string bin maps to { <name-basename>: <path> }.
        assert_eq!(m.bin.get("svelte"), Some(&"./cli.js".to_string()));
        assert_eq!(
            m.tarball_url,
            "https://registry.npmjs.org/@neoconfetti/svelte/-/svelte-2.2.2.tgz"
        );
    }

    #[test]
    fn non_default_registry_entry_reconstructs_manifest_with_lockfile_url() {
        let values = vec![
            json!("foo@1.0.0"),
            json!("https://npm.example.com/foo/-/foo-1.0.0.tgz"),
            json!({ "dependencies": { "bar": "^1.0.0" } }),
            json!(SHA),
        ];

        let pkg = PackageDeserializer::deserialize_package("foo".into(), values).unwrap();
        let m = pkg
            .manifest
            .expect("non-default-registry npm entries carry a manifest too");

        // The explicit lockfile URL is used verbatim as the tarball URL.
        assert_eq!(m.tarball_url, "https://npm.example.com/foo/-/foo-1.0.0.tgz");
        assert_eq!(m.dependencies.get("bar"), Some(&"^1.0.0".to_string()));
    }
}
