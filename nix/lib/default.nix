{ inputs, ... }:
let
  eachSystem = inputs.nixpkgs.lib.genAttrs (import inputs.systems);
in
eachSystem (
  system:
  let
    pkgs = inputs.nixpkgs.legacyPackages.${system};
  in
  {
    mkBunDerivation = pkgs.callPackage ./mkBunDerivation.nix { };
  }
)
