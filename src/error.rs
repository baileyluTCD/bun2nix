//! Errors which may occur during the running of this program
//!
//! This module contains two items:
//! - A giant unified error type `Error`
//! - An alias for `std::result::Result<T, E>` with that error for convenience

use thiserror::Error;

/// Result alias for Errors which occur in `bun2nix`
pub type Result<T> = std::result::Result<T, Error>;

#[allow(missing_docs)]
#[derive(Error, Debug)]
/// Errors which occur in `bun2nix`
pub enum Error {
    #[error("Failed to parse lockfile as JSONC (specified here: https://github.com/oven-sh/bun/issues/11863): {}. Please make sure your bun lockfile is formatted correctly, try deleting it and running `bun install` again to produce a fresh one.", 0)]
    ParseJsonc(#[from] jsonc_parser::errors::ParseError),
    #[error("Failed to parse lockfile JSONC as rust type: {}.", 0)]
    ParseRustType(#[from] serde_json::Error),
    #[error(
        "Failed to parse empty lockfile, make sure you are providing a file with text contents."
    )]
    NoJsoncValue,
    #[error("Missing @ for package name and version declaration. Make sure all versions in your bun lockfile are formatted properly or try deleting it and running `bun install` to produce a fresh one.")]
    NoAtInPackageIdentifier,
    #[error("Error occurred in nix-prefetch command: {}.", 0)]
    Prefetch(#[from] std::io::Error),
    #[error("Prefetch command returned an error code. STDERR: {}", 0)]
    PrefetchStderr(String),
    #[error("Cache table did not have value for: {}", 0)]
    CacheTable(String),
    #[error("Error parsing UTF8 nix-prefetch stdout: {}.", 0)]
    UTF8Parse(#[from] std::string::FromUtf8Error),
    #[error(
        "Unsupported lockfile version: '{}'. Consider updating your local package or contributing to `bun2nix` if this version hasn't been supported yet.",
        0
    )]
    UnsupportedLockfileVersion(u8),
    #[error("Error connecting to database: '{}'", 0)]
    DatabaseConnection(#[from] sqlx::Error),
    #[error("Error migrating database: '{}'", 0)]
    DatabaseMigration(#[from] sqlx::migrate::MigrateError),
    #[error("Error joining tokio task: '{}'", 0)]
    JoinError(#[from] tokio::task::JoinError),
    #[error("Failed to render template: '{}'", 0)]
    TemplateError(#[from] rinja::Error),
}
