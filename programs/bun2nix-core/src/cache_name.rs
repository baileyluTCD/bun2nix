//! Cache-folder naming logic ported from `programs/cache-entry-creator/src/main.zig`.
//!
//! Produces the bun on-disk cache directory name for a given package identifier.
//! See bun's `PackageManagerDirectories.zig` for the original source.

use crate::wyhash::wyhash11;

const WYHASH_SEED: u64 = 0;

/// Dispatch: select the correct naming function based on the identifier prefix.
///
/// Mirrors `cachedFolderPrintBasename` from the Zig implementation.
pub fn cached_folder_print_basename(input: &str, registry: Option<&str>) -> String {
    if input.starts_with("tarball:") {
        cached_tarball_folder_print_basename(input)
    } else if input.starts_with("github:") {
        cached_github_folder_print_basename(input)
    } else if input.starts_with("git:") {
        cached_git_folder_print_basename(input)
    } else {
        cached_npm_package_folder_print_basename(input, registry)
    }
}

/// Produce a correct bun cache folder name for a given npm identifier.
///
/// Ported from `cachedNpmPackageFolderPrintBasename` in `main.zig`.
///
/// When a non-default registry is used, the format includes the registry hostname:
/// e.g., `@scope/pkg@1.0.0@@npm.pkg.github.com@@@1`.
///
/// Pre-release components (after `-`) are hashed with wyhash11, formatted **lowercase**.
/// Build-metadata components (after `+`) are hashed with wyhash11, formatted **uppercase**.
pub fn cached_npm_package_folder_print_basename(pkg: &str, registry: Option<&str>) -> String {
    // Suffix is "@@{registry}@@@1" for non-default registries, or "@@@1" for default.
    let suffix = if let Some(reg) = registry {
        format!("@@{}@@@1", reg)
    } else {
        "@@@1".to_string()
    };

    // Find the last '@' to split name from version (handles scoped packages like @scope/pkg@ver).
    let Some(version_start) = pkg.rfind('@') else {
        return format!("{}{}", pkg, suffix);
    };
    let name = &pkg[..version_start];
    let ver = &pkg[version_start..]; // includes leading '@'

    // Handle pre-release: ver contains '-' before any '+'
    if let Some(pre_idx) = ver.find('-') {
        let version = &ver[..pre_idx]; // e.g. "@1.2.3"
        let pre_and_build = &ver[pre_idx + 1..]; // e.g. "beta.1+build.123"

        if let Some(build_idx) = pre_and_build.find('+') {
            let pre = &pre_and_build[..build_idx];
            let build = &pre_and_build[build_idx + 1..];
            // pre-release: lowercase; build-metadata: uppercase — match Zig {x:0>16}/{X:0>16}
            return format!(
                "{}{}-{:016x}+{:016X}{}",
                name,
                version,
                wyhash11(WYHASH_SEED, pre.as_bytes()),
                wyhash11(WYHASH_SEED, build.as_bytes()),
                suffix,
            );
        }

        return format!(
            "{}{}-{:016x}{}",
            name,
            version,
            wyhash11(WYHASH_SEED, pre_and_build.as_bytes()),
            suffix,
        );
    }

    // Handle build-metadata only (no pre-release '-')
    if let Some(build_idx) = ver.find('+') {
        let version = &ver[..build_idx]; // e.g. "@1.2.3"
        let build = &ver[build_idx + 1..]; // e.g. "build.123"
        // build-metadata: uppercase
        return format!(
            "{}{}+{:016X}{}",
            name,
            version,
            wyhash11(WYHASH_SEED, build.as_bytes()),
            suffix,
        );
    }

    // Plain version — no hashing needed.
    format!("{}{}", pkg, suffix)
}

/// Produce a correct bun cache folder name for a given tarball dependency.
///
/// Ported from `cachedTarballFolderPrintBasename` in `main.zig`.
pub fn cached_tarball_folder_print_basename(url: &str) -> String {
    let without_pre = &url["tarball:".len()..];
    format!(
        "@T@{:016x}@@@1",
        wyhash11(WYHASH_SEED, without_pre.as_bytes())
    )
}

/// Produce a correct bun cache folder name for a given github dependency.
///
/// Ported from `cachedGithubFolderPrintBasename` in `main.zig`.
pub fn cached_github_folder_print_basename(url: &str) -> String {
    let without_pre = &url["github:".len()..];
    format!("@GH@{}@@@1", without_pre)
}

/// Produce a correct bun cache folder name for a given git dependency.
///
/// Ported from `cachedGitFolderPrintBasename` in `main.zig`.
pub fn cached_git_folder_print_basename(url: &str) -> String {
    let without_pre = &url["git:".len()..];
    format!("@G@{}", without_pre)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Verbatim Zig test vectors ────────────────────────────────────────────
    // Copied from `programs/cache-entry-creator/src/main.zig`, converted to
    // Rust `#[test]` form.  These are the fidelity gate — casing must match exactly.

    #[test]
    fn cached_npm_package_folder_print_basename_fn() {
        let cases: &[(&str, Option<&str>, &str)] = &[
            // Without registry (default npm registry)
            (
                "react@1.2.3-beta.1+build.123",
                None,
                "react@1.2.3-c0734e9369ab610d+F48F05ED5AABC3A0@@@1",
            ),
            (
                "tailwindcss@4.0.0-beta.9",
                None,
                "tailwindcss@4.0.0-73c5c46324e78b9b@@@1",
            ),
            (
                "react@1.2.3+build.123",
                None,
                "react@1.2.3+F48F05ED5AABC3A0@@@1",
            ),
            ("react@1.2.3", None, "react@1.2.3@@@1"),
            ("undici-types@6.20.0", None, "undici-types@6.20.0@@@1"),
            (
                "@types/react-dom@19.0.4",
                None,
                "@types/react-dom@19.0.4@@@1",
            ),
            (
                "react-compiler-runtime@19.0.0-beta-e552027-20250112",
                None,
                "react-compiler-runtime@19.0.0-0f3fc645a5103715@@@1",
            ),
            // With registry (non-default registry like GitHub Packages)
            (
                "@scope/package@1.0.0",
                Some("npm.pkg.github.com"),
                "@scope/package@1.0.0@@npm.pkg.github.com@@@1",
            ),
            (
                "private-pkg@2.0.0",
                Some("my.registry.com"),
                "private-pkg@2.0.0@@my.registry.com@@@1",
            ),
            // With registry and pre-release version
            (
                "@scope/pkg@1.0.0-beta.1",
                Some("npm.pkg.github.com"),
                "@scope/pkg@1.0.0-c0734e9369ab610d@@npm.pkg.github.com@@@1",
            ),
            // With registry and build metadata
            (
                "@scope/pkg@1.0.0+build.123",
                Some("npm.pkg.github.com"),
                "@scope/pkg@1.0.0+F48F05ED5AABC3A0@@npm.pkg.github.com@@@1",
            ),
        ];
        for &(input, registry, expected) in cases {
            let result = cached_npm_package_folder_print_basename(input, registry);
            assert_eq!(
                result, expected,
                "npm basename mismatch: input={:?} registry={:?}",
                input, registry
            );
        }
    }

    #[test]
    fn cached_tarball_folder_print_basename_fn() {
        let cases: &[(&str, &str)] = &[(
            "tarball:https://registry.npmjs.org/zod/-/zod-3.21.4.tgz",
            "@T@3be02e19198e30ee@@@1",
        )];
        for &(input, expected) in cases {
            let result = cached_tarball_folder_print_basename(input);
            assert_eq!(
                result, expected,
                "tarball basename mismatch: input={:?}",
                input
            );
        }
    }

    #[test]
    fn cached_github_folder_print_basename_fn() {
        let cases: &[(&str, &str)] = &[(
            "github:colinhacks-zod-f9bbb50",
            "@GH@colinhacks-zod-f9bbb50@@@1",
        )];
        for &(input, expected) in cases {
            let result = cached_github_folder_print_basename(input);
            assert_eq!(
                result, expected,
                "github basename mismatch: input={:?}",
                input
            );
        }
    }

    #[test]
    fn cached_git_folder_print_basename_fn() {
        let cases: &[(&str, &str)] = &[(
            "git:ee100d81f12ae315a81c2a664979a6cc1bce99a2",
            "@G@ee100d81f12ae315a81c2a664979a6cc1bce99a2",
        )];
        for &(input, expected) in cases {
            let result = cached_git_folder_print_basename(input);
            assert_eq!(result, expected, "git basename mismatch: input={:?}", input);
        }
    }

    #[test]
    fn dispatcher_routes_correctly() {
        // Verify the dispatcher selects the right function for each prefix.
        assert!(
            cached_folder_print_basename("tarball:https://foo.com/pkg.tgz", None)
                .starts_with("@T@")
        );
        assert!(cached_folder_print_basename("github:owner-repo-sha", None).starts_with("@GH@"));
        assert!(cached_folder_print_basename("git:abc123", None).starts_with("@G@"));
        assert!(cached_folder_print_basename("react@1.0.0", None).ends_with("@@@1"));
    }
}
