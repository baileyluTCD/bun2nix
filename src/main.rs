use bun2nix::convert_lockfile_to_nix_expression;

use std::{
    fs::{self, File},
    io::Write,
};

use clap::Parser;

/// Convert Bun (v1.2+) packages to Nix expressions
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The Bun (v1.2+) lockfile to use to produce the Nix expression
    #[arg(short, long, default_value_t = String::from("./bun.lock"))]
    lock_file: String,

    // The output file to write to
    // If no file location is provided, print to stdout instead
    #[arg(short, long)]
    output_file: Option<String>,
}

fn main() {
    let args = Args::parse();

    let lockfile = fs::read_to_string(&args.lock_file)
        .unwrap_or_else(|_| panic!("Could not find lockfile at {}. Try changing the file path to point to one, or create one with `bun install` on a version of bun above v1.2. See https://bun.sh/docs/install/lockfile to find out more information about the textual lockfile.", args.lock_file));

    let nix = convert_lockfile_to_nix_expression(lockfile).unwrap();

    match args.output_file {
        Some(output_file) => {
            let mut output = File::create(output_file).unwrap();
            write!(output, "{}", nix).unwrap();
        }
        None => println!("{}", nix),
    };
}
