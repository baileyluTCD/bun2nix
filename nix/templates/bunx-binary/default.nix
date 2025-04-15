{ bun2nix, ... }:
bun2nix.mkBunDerivation {
  name = "binary-exec-bun2nix-example";
  version = "1.0.0";

  src = ./.;

  bunNix = ./bun.nix;

  # Verify that the cowsay binary was installed as expected and is runnable
  preBuild = ''
    bun ./node_modules/.bin/cowsay "hello 1"
    bunx cowsay "Hello Nix logs!"
  '';

  index = ./index.ts;
}
