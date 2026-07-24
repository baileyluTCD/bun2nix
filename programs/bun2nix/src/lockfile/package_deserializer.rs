use crate::{
    Package,
    error::{Error, Result},
    package::Fetcher,
};

mod prefetch;
pub use prefetch::Prefetch;

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
        // - metadata: object with dependencies, bin, etc.
        // - hash: integrity hash (sha512-...)

        let npm_identifier_raw = swap_remove_value(&mut self.values, 0);
        // After swap_remove(0): [hash, tarball_url, meta]

        let hash = swap_remove_value(&mut self.values, 0);
        // After swap_remove(0): [meta, tarball_url]

        // Get the tarball URL from what's now at index 1
        // (originally at index 1, but the metadata swapped in at index 0)
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

        Ok(Package::new(npm_identifier_raw, fetcher))
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
}
