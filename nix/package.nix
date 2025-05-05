{
  pkgs,
  ...
}:
let
  cargoTOML = builtins.fromTOML (builtins.readFile ../Cargo.toml);
in
pkgs.rustPlatform.buildRustPackage {
  pname = cargoTOML.package.name;
  version = cargoTOML.package.version;

  src = ../.;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  meta = with pkgs.lib; {
    description = "A fast rust based bun lockfile to nix expression converter.";
    homepage = "https://github.com/baileyluTCD/bun2nix";
    license = licenses.mit;
    maintainers = [ "baileylu@tcd.ie" ];
  };
}
