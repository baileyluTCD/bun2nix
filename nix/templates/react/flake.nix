{
  description = "Bun2Nix react sample";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    bun2nix.url = "github:baileyluTCD/bun2nix?tag=1.4.1";
    bun2nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  # Use the cached version of bun2nix from the garnix cli
  nixConfig = {
    extra-substituters = [
      "https://cache.nixos.org"
      "https://cache.garnix.io"
    ];
    extra-trusted-public-keys = [
      "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
      "cache.garnix.io:CTFPyKSLcx5RMJKfLo5EEPUObbA78b0YQ2DTCJXqr9g="
    ];
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      bun2nix,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Call our package with the bun2nix library functions
        react-site = pkgs.callPackage ./default.nix {
          inherit (bun2nix.lib.${system}) mkBunDerivation;
        };
      in
      {
        packages.default = react-site;

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            bun
            # Add the bun2nix binary to our devshell
            bun2nix.packages.${system}.default
          ];

          shellHook = ''
            bun install --frozen-lockfile
          '';
        };
      }
    );
}
