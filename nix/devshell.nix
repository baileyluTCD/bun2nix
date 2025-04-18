{ pkgs, ... }:
pkgs.mkShell {
  packages = with pkgs; [
    # Rust dependencies
    rustc
    cargo
    rustfmt
    clippy
    mold

    # Database
    sqlx-cli
    sqlite

    # SSL
    pkg-config
    openssl

    # Docs
    mdbook

    # Javascript dependencies
    bun
  ];

  env = with pkgs; {
    RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
    LD_LIBRARY_PATH = lib.makeLibraryPath [ openssl ];
    DATABASE_URL = "sqlite://.cache/bun2nix";
  };

  shellHook = ''
    mkdir .cache
    touch .cache/bun2nix
  '';
}
