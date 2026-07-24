//! Library for implementing parsing and conversion of [Bun](https://bun.sh/) lock files into a
//! [Nix](https://en.wikipedia.org/wiki/Nix_(package_manager)) expression.

#![warn(missing_docs)]

pub mod error;
pub mod lockfile;
pub mod nix_expression;
pub mod options;
pub mod package;

pub use error::{Error, Result};
pub use lockfile::Lockfile;
use nix_expression::NixExpression;
pub use options::Options;
pub use package::Package;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// # Convert Bun Lockfile to a Nix expression
///
/// Takes a string input of the contents of a bun lockfile and converts it into a ready to use Nix expression which fetches the packages
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[cfg_attr(target_arch = "wasm32", no_mangle)]
pub fn convert_lockfile_to_nix_expression(contents: String, options: Options) -> Result<String> {
    let packages = build_packages(&contents)?;
    render_packages(packages, options)
}

/// # Build Packages from a Lockfile
///
/// Parses a bun lockfile and produces the sorted, de-duplicated list of
/// [`Package`]s it describes. Every npm-registry package carries a manifest
/// reconstructed from the lockfile's inline metadata.
pub fn build_packages(contents: &str) -> Result<Vec<Package>> {
    let lockfile = contents.parse::<Lockfile>()?;

    if lockfile.lockfile_version != 1 {
        return Err(Error::UnsupportedLockfileVersion(lockfile.lockfile_version));
    };

    // Workspace name → directory, for resolving nested file-dependency paths:
    // bun records a `<workspace-name>/<pkg>` entry's path relative to that
    // workspace's directory, but `bun.nix` paths resolve from the project root.
    let workspace_dirs: Vec<(String, String)> = lockfile
        .workspaces
        .iter()
        .filter_map(|(dir, ws)| ws.name.clone().map(|name| (name, dir.clone())))
        .collect();

    let mut packages = lockfile.packages();
    packages.sort();
    packages.dedup_by(|a, b| a.name == b.name);

    for package in &mut packages {
        if let package::Fetcher::CopyToStore { path } = &mut package.fetcher {
            // Longest matching `<workspace-name>/` prefix wins; entries whose
            // key IS a workspace name (the workspaces themselves) don't match.
            let parent_dir = workspace_dirs
                .iter()
                .filter(|(name, _)| {
                    !name.is_empty()
                        && package.name.len() > name.len() + 1
                        && package.name.starts_with(name.as_str())
                        && package.name.as_bytes()[name.len()] == b'/'
                })
                .max_by_key(|(name, _)| name.len())
                .map(|(_, dir)| dir.as_str());
            if let Some(dir) = parent_dir
                && !dir.is_empty()
            {
                *path = normalize_path(&format!("{dir}/{path}"));
            }
        }
    }

    Ok(packages)
}

/// # Render Packages to a Nix Expression
///
/// Renders a (possibly manifest-enriched) package list into the final `bun.nix`
/// text using the supplied [`Options`].
pub fn render_packages(packages: Vec<Package>, options: Options) -> Result<String> {
    NixExpression::new(packages)?.render_with_options(options)
}

/// Collapse `.` and `..` segments lexically (`a/b/../c` → `a/c`).
fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.last().is_none_or(|last| *last == "..") {
                    parts.push("..");
                } else {
                    parts.pop();
                }
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    // A vendored tarball nested under a workspace ("<ws-name>/<pkg>") is
    // recorded relative to the workspace dir; bun.nix needs it root-relative.
    #[test]
    fn nested_file_dep_paths_resolve_against_workspace_dir() {
        let lock = r#"{
  "lockfileVersion": 1,
  "workspaces": {
    "": { "name": "root" },
    "packages/app": { "name": "@oc/app" },
    "packages/ui": { "name": "@oc/ui" }
  },
  "packages": {
    "@oc/app": ["@oc/app@workspace:packages/app"],
    "@oc/ui": ["@oc/ui@workspace:packages/ui"],
    "@oc/app/@oc/client": ["@oc/client@vendor/client-1.0.0.tgz", {}, "sha512-AAAA"],
    "@oc/ui/@oc/client": ["@oc/client@../app/vendor/client-1.0.0.tgz", {}, "sha512-AAAA"],
  }
}"#;
        let pkgs = build_packages(lock).unwrap();
        let path_of = |name: &str| {
            let p = pkgs.iter().find(|p| p.name == name).unwrap();
            match &p.fetcher {
                package::Fetcher::CopyToStore { path } => path.clone(),
                other => panic!("expected CopyToStore, got {other:?}"),
            }
        };

        assert_eq!(
            path_of("@oc/app/@oc/client"),
            "packages/app/vendor/client-1.0.0.tgz"
        );
        assert_eq!(
            path_of("@oc/ui/@oc/client"),
            "packages/app/vendor/client-1.0.0.tgz"
        );
        // Workspace entries themselves stay untouched.
        assert_eq!(path_of("@oc/app"), "packages/app");
    }
}
