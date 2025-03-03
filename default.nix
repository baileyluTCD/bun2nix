{
  lib,
  rustPlatform,
}: let
  cargoTOML = builtins.fromTOML (builtins.readFile ./Cargo.toml);
in
  rustPlatform.buildRustPackage {
    pname = cargoTOML.package.name;
    version = cargoTOML.package.version;

    src = ./.;

    # Disable network using tests
    checkFlags = [
      "--skip=test_parse_minimal_lockfile"
      "--skip=test_parse_react_lockfile"
      "--skip=test_prefetch_packages"
    ];

    cargoLock = {
      lockFile = ./Cargo.lock;
    };

    meta = with lib; {
      description = "A fast rust based bun lockfile to nix expression converter.";
      homepage = "https://github.com/baileyluTCD/bun2nix";
      license = licenses.mit;
      maintainers = ["baileylu@tcd.ie"];
    };
  }
