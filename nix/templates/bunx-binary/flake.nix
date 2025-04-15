{
  description = "Bun2Nix bunx binary sample";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    bun2nix = {
      url = "github:baileyluTCD/bun2nix";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      ...
    }@inputs:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        bun2nix = inputs.bun2nix.defaultPackage.${system};

        minimal = pkgs.callPackage ./default.nix { inherit bun2nix; };
      in
      {
        defaultPackage = minimal;

        defaultApp = {
          type = "app";
          program = "${minimal}/bin/binary-exec-bun2nix-example";
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            bun
            bun2nix.bin
          ];
        };
      }
    );
}
