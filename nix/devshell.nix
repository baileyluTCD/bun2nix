{ pkgs, ... }:
pkgs.mkShell {
  packages = with pkgs; [
    # Rust dependencies
    rustc
    cargo
    rustfmt
    clippy
    mold

    # Docs
    mdbook

    # Javascript dependencies
    bun
  ];

  env = {
    RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
  };
}
