{ inputs, ... }:
let
  eachSystem = inputs.nixpkgs.lib.genAttrs (import inputs.systems);
in
eachSystem (
  system:
  let
    pkgs = inputs.nixpkgs.legacyPackages.${system};
  in
  rec {
    mkBunNodeModules = pkgs.callPackage ./mkBunNodeModules.nix { };
    writeBunScriptBin = pkgs.callPackage ./writeBunScriptBin.nix { };

    mkBunDerivation = pkgs.callPackage ./mkBunDerivation.nix { inherit mkBunNodeModules; };

    treefmt = inputs.treefmt-nix.lib.evalModule pkgs (import ./treefmtConfig.nix);
  }
)
