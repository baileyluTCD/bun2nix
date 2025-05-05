{ mkBunDerivation, ... }:
mkBunDerivation {
  pname = "git-deps-bun2nix-example";
  version = "1.0.0";

  src = ./.;

  bunNix = ./bun.nix;

  index = "index.ts";
}
