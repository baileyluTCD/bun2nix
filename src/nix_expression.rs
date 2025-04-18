//! This module handles construction of the rendered nix code as the output

mod nix_escaper;

pub use crate::package::NormalizedBinary;
pub use nix_escaper::NixEscaper;
use rinja::Template;

use crate::{
    package::{Fetched, Normalized},
    Package,
};

/// # Nix Expression
///
/// A chunk of nix code to be written to stdout or a file
#[derive(Template)]
#[template(path = "output.nix_template")]
pub struct NixExpression {
    packages: Vec<Package<Normalized>>,
}

impl NixExpression {
    /// # New Nix Expression
    ///
    /// Produce a new, ready to render, nix expression from a fetch package list
    pub fn new(packages: Vec<Package<Fetched>>) -> Self {
        Self {
            packages: packages.into_iter().map(|pkg| pkg.normalize()).collect(),
        }
    }
}
