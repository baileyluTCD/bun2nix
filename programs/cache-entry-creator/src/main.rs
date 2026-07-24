//! `cache_entry_creator` — bun cache-entry tool (Rust rewrite of the Zig binary).
//!
//! Two subcommands:
//! - `symlink` — compute the bun cache folder name and create a directory symlink
//!   (preserves the Zig behavior; same CLI flags as the original tool).
//! - `manifest` — read a `Vec<EntryMeta>` JSON and write `.npm` manifest files.

use std::{
    collections::BTreeMap,
    fs,
    io,
    path::{Path, PathBuf},
};

use bun2nix_core::{
    cache_name::cached_folder_print_basename,
    manifest::{
        build::build_manifest,
        default_url_hash,
        manifest_file_name,
        meta::{EntryMeta, PackageMeta, VersionMeta},
        serialize,
        DEFAULT_REGISTRY_HREF_LEN,
    },
};
use clap::{Parser, Subcommand};

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "cache_entry_creator",
    about = "Tool for producing correctly named and positioned bun cache entries."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a bun cache directory symlink for an npm/tarball/git package.
    ///
    /// Computes the bun cache folder name for `--name` and creates a directory
    /// symlink from `<out>/<basename>` to `--package`.  This is a direct port
    /// of the Zig tool's behavior; the Nix caller needs no flag-site changes.
    Symlink {
        /// The $out directory to write the symlink into (created if absent).
        #[arg(long)]
        out: PathBuf,
        /// The package name+version as found in `bun.lock`, e.g. `react@18.3.1`.
        #[arg(long)]
        name: String,
        /// Absolute path to the extracted package contents to symlink.
        #[arg(long)]
        package: PathBuf,
        /// Optional registry hostname for non-default registries,
        /// e.g. `npm.pkg.github.com`.
        #[arg(long)]
        registry: Option<String>,
    },

    /// Write bun `.npm` manifest cache files for a set of package entries.
    ///
    /// Reads `--meta` as a `Vec<EntryMeta>` JSON, groups entries by package
    /// name, builds a manifest per package, and writes `<wyhash(name)>.npm`
    /// files into `--out`.
    Manifest {
        /// The directory to write `.npm` files into (created if absent).
        #[arg(long)]
        out: PathBuf,
        /// Path to the JSON file containing `Vec<EntryMeta>`.
        #[arg(long)]
        meta: PathBuf,
    },
}

// ─── Main ────────────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Symlink { out, name, package, registry } => {
            run_symlink(&out, &name, &package, registry.as_deref())?;
        }
        Commands::Manifest { out, meta } => {
            run_manifest(&out, &meta)?;
        }
    }
    Ok(())
}

// ─── symlink subcommand ───────────────────────────────────────────────────────

/// Core logic for `symlink` mode.
///
/// Computes the bun cache folder name for `name`, creates parent dirs under
/// `out`, and creates a **directory symlink** from `<out>/<basename>` to
/// `package`.
///
/// Mirrors `PkgLinker::create_cache_entry` from the Zig implementation.
fn run_symlink(
    out: &Path,
    name: &str,
    package: &Path,
    registry: Option<&str>,
) -> io::Result<()> {
    eprintln!("Creating entry for `{}`...", name);

    let basename = cached_folder_print_basename(name, registry);
    let link_path = out.join(&basename);

    // Create parent directory (handles scoped packages like @types/foo which
    // produce a basename of `@types/foo@ver@@@1`, needing a `@types/` subdir).
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)?;
    }

    eprintln!("Link out path: `{}`.", link_path.display());

    // Create directory symlink from the computed location → the package store path.
    #[cfg(unix)]
    std::os::unix::fs::symlink(package, &link_path)?;

    #[cfg(not(unix))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlink mode requires a Unix system",
    ));

    eprintln!("Successfully created cache entry symlink for `{}`.", name);
    Ok(())
}

// ─── manifest subcommand ──────────────────────────────────────────────────────

/// Core logic for `manifest` mode.
///
/// Reads `meta_path` as `Vec<EntryMeta>`, sets each entry's
/// `manifest.integrity` from its `hash` field (the SRI is reused as npm
/// integrity; `build_manifest` decodes it), groups by package name, builds a
/// [`PackageManifest`], and serializes it to
/// `<out>/<manifest_file_name(name)>`.
///
/// v1 scope: entries with `registry == Some(_)` are skipped with a diagnostic.
fn run_manifest(out: &Path, meta_path: &Path) -> io::Result<()> {
    let content = fs::read_to_string(meta_path)?;
    let mut entries: Vec<EntryMeta> =
        serde_json::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Group entries by package name, populating integrity from the Nix hash.
    let mut by_name: BTreeMap<String, Vec<VersionMeta>> = BTreeMap::new();

    for entry in &mut entries {
        // Re-use the SRI hash string as the npm integrity field.
        entry.manifest.integrity = entry.hash.clone();

        // Parse the package name from "name@version".
        // `rsplit_once('@')` correctly handles scoped packages like
        // "@scope/pkg@1.2.3" → ("@scope/pkg", "1.2.3").
        let pkg_name = match entry.name_version.rsplit_once('@') {
            Some((name, _ver)) => name.to_string(),
            None => entry.name_version.clone(),
        };

        // v1: skip non-default registry entries. Non-default-registry packages do not
        // receive a `manifest` attr in `bun.nix` (only default-registry entries are
        // enriched), so in v1 this branch is effectively unreachable; it exists as a
        // defensive, documented limitation. Non-default-registry manifest support is a
        // future extension.
        if let Some(ref reg) = entry.registry {
            eprintln!(
                "cache_entry_creator: skipping {}: non-default registry ({}) manifests are not supported in v1",
                entry.name_version, reg
            );
            continue;
        }

        by_name.entry(pkg_name).or_default().push(entry.manifest.clone());
    }

    fs::create_dir_all(out)?;

    for (name, versions) in by_name {
        let pkg_meta = PackageMeta { name: name.clone(), versions };
        let manifest = build_manifest(&pkg_meta);
        let filename = manifest_file_name(&name);
        let out_path = out.join(&filename);
        let mut file = fs::File::create(&out_path)?;
        serialize::write(&manifest, default_url_hash(), DEFAULT_REGISTRY_HREF_LEN, &mut file)?;
        eprintln!("Wrote manifest: {}", out_path.display());
    }

    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bun2nix_core::manifest::{manifest_file_name, read};

    /// Create a temporary directory for a test, unique per process + test name.
    fn temp_test_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("cec-test-{}-{}", label, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    // ── manifest mode integration test ──────────────────────────────────────

    #[test]
    fn manifest_mode_neoconfetti_round_trip() {
        // Build a small EntryMeta JSON for @neoconfetti/svelte@2.2.2 using
        // data from programs/bun2nix/tests/fixtures/neoconfetti.json.
        let entry_meta_json = r#"[
          {
            "name_version": "@neoconfetti/svelte@2.2.2",
            "hash": "sha512-E7xCFVEEm5Ctnj2udTJy1b9oaTvjz1zi1mYdEtE8rB5BVwq6kHisosDS+zdWN5PMfEMjtbsOV9Cl6tsNSAD1sA==",
            "registry": null,
            "manifest": {
              "version": "2.2.2",
              "tarball_url": "https://registry.npmjs.org/@neoconfetti/svelte/-/svelte-2.2.2.tgz",
              "integrity": "",
              "dependencies": {},
              "peer_dependencies": {
                "svelte": "^3.0.0 || ^4.0.0 || ^5.0.0"
              },
              "optional_dependencies": {},
              "optional_peers": [],
              "bin": {},
              "os": [],
              "cpu": [],
              "has_install_script": false
            }
          }
        ]"#;

        let out_dir = temp_test_dir("manifest-neoconfetti");
        let meta_path = out_dir.join("meta.json");
        fs::write(&meta_path, entry_meta_json).unwrap();

        // Run manifest mode.
        run_manifest(&out_dir, &meta_path).expect("manifest mode should succeed");

        // (a) Assert the output file is named exactly <wyhash(name)>.npm.
        let expected_filename = manifest_file_name("@neoconfetti/svelte");
        let out_file = out_dir.join(&expected_filename);
        assert!(
            out_file.exists(),
            "expected output file `{}` does not exist; dir contents: {:?}",
            out_file.display(),
            fs::read_dir(&out_dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<Vec<_>>()
        );

        // (b) Round-trip: read the file back and verify key fields.
        let bytes = fs::read(&out_file).unwrap();
        let rm = read(&bytes).expect("should parse .npm file");

        // Package name.
        let name_bytes = rm.name();
        assert_eq!(
            std::str::from_utf8(&name_bytes).unwrap(),
            "@neoconfetti/svelte",
            "package name mismatch"
        );

        // public_max_age == u32::MAX (never expire).
        assert_eq!(
            rm.public_max_age(),
            u32::MAX,
            "public_max_age should be u32::MAX"
        );

        // Version 2.2.2 present with the expected peer dependency.
        let pv = rm.find_version("2.2.2").expect("version 2.2.2 should be present");
        let peers = pv.peer_dependencies();
        assert_eq!(
            peers.get("svelte").map(|s| s.as_str()),
            Some("^3.0.0 || ^4.0.0 || ^5.0.0"),
            "svelte peer dependency range mismatch"
        );
    }

    // ── symlink mode test ────────────────────────────────────────────────────

    #[test]
    #[cfg(unix)]
    fn symlink_mode_creates_link_at_expected_basename() {
        let out_dir = temp_test_dir("symlink-react");

        // Create a fake "package" directory to symlink to.
        let pkg_dir = temp_test_dir("symlink-react-pkg");
        fs::write(pkg_dir.join("package.json"), r#"{"name":"react"}"#).unwrap();

        // Run symlink mode.
        run_symlink(&out_dir, "react@18.3.1", &pkg_dir, None)
            .expect("symlink mode should succeed");

        // Assert the symlink exists at the expected basename.
        let expected_basename =
            bun2nix_core::cache_name::cached_folder_print_basename("react@18.3.1", None);
        let link_path = out_dir.join(&expected_basename);

        assert!(
            link_path.exists() || link_path.is_symlink(),
            "symlink not found at `{}`",
            link_path.display()
        );

        // The symlink should point at pkg_dir.
        let target = fs::read_link(&link_path).expect("read_link");
        assert_eq!(target, pkg_dir, "symlink target mismatch");
    }

    #[test]
    #[cfg(unix)]
    fn symlink_mode_scoped_package_creates_parent_dir() {
        let out_dir = temp_test_dir("symlink-scoped");
        let pkg_dir = temp_test_dir("symlink-scoped-pkg");
        fs::write(pkg_dir.join("package.json"), r#"{"name":"@types/node"}"#).unwrap();

        run_symlink(&out_dir, "@types/node@20.0.0", &pkg_dir, None)
            .expect("scoped symlink mode should succeed");

        let expected_basename =
            bun2nix_core::cache_name::cached_folder_print_basename("@types/node@20.0.0", None);
        // A parent directory (@types/) must be created because the cached basename
        // contains a '/' from the scoped package name (@types/node).
        let link_path = out_dir.join(&expected_basename);
        assert!(
            link_path.exists() || link_path.is_symlink(),
            "symlink not found at `{}`; basename=`{}`",
            link_path.display(),
            expected_basename
        );
    }
}
