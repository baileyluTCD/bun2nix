//! Main entry point for the `bun2nix` command line tool, which makes calls to the library for the
//! majority of the actual processing

#![warn(missing_docs)]

use bun2nix::{Options, RegistryConfig, Result, build_packages, render_packages};
use log::{error, warn};

use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use clap::Parser;
use env_logger::Env;

/// Convert Bun (v1.2+) packages to Nix expressions
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// The Bun (v1.2+) lockfile to use to produce the Nix expression.
    #[arg(short, long, default_value = "./bun.lock")]
    lock_file: PathBuf,

    /// The output file to write to -
    /// if no file location is provided, print to stdout instead.
    #[arg(short, long)]
    output_file: Option<PathBuf>,

    /// The prefix to use when copying workspace or file packages
    #[arg(short, long, default_value = "./")]
    copy_prefix: String,
}

/// Read an optional project-local config file. Absence is normal; any other
/// IO failure is surfaced as a warning because silently ignoring the file
/// would generate manifest cache keys bun won't find at install time.
fn read_optional_config(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(content) => Some(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            warn!("Failed to read {}: {e}", path.display());
            None
        }
    }
}

fn main() {
    let log_env = Env::default().default_filter_or("warn");
    env_logger::Builder::from_env(log_env).init();

    match run() {
        Ok(()) => (),
        Err(err) => {
            error!("\n{err}\n");

            std::process::exit(1)
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let lockfile = fs::read_to_string(&cli.lock_file)?;

    // Project-local config only: ./bunfig.toml then ./.npmrc next to the
    // lockfile (.npmrc overrides per key). Global/env/CLI layers are
    // deliberately not read — the Nix sandbox's bun can't see them either,
    // and the .npm cache key must match what bun computes there.
    let dir = cli.lock_file.parent().unwrap_or(Path::new("."));
    let bunfig = read_optional_config(&dir.join("bunfig.toml"));
    let npmrc = read_optional_config(&dir.join(".npmrc"));
    let registry_config = RegistryConfig::parse(bunfig.as_deref(), npmrc.as_deref())?;

    let packages = build_packages(&lockfile, &registry_config)?;

    let nix = render_packages(
        packages,
        Options {
            copy_prefix: cli.copy_prefix,
        },
    )?;

    if let Some(output_file) = cli.output_file {
        let mut output = File::create(output_file)?;
        write!(output, "{nix}")?;
    } else {
        println!("{nix}");
    }

    Ok(())
}
