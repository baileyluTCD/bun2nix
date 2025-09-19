{ pkgs, ... }:
pkgs.mkShell (
  let
    # Mold does not support MacOS
    enableMold = !pkgs.stdenv.isDarwin;
  in
  {
    packages =
      with pkgs;
      [
        # Rust dependencies
        rustc
        cargo
        rustfmt
        clippy

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
      ]
      ++ pkgs.lib.optional enableMold pkgs.mold;

    env =
      with pkgs;
      {
        LD_LIBRARY_PATH = lib.makeLibraryPath [ openssl ];
        DATABASE_URL = "sqlite://.cache/bun2nix";
      }
      // lib.optionalAttrs enableMold {
        RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
      };

    shellHook = ''
      mkdir -p .cache
      touch .cache/bun2nix
    '';
  }
)
