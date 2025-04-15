# In a pre-existing flake

To install bun2nix in an already existing bun project with a `flake.nix`, the following steps are recommended:

## 1. Source the bun2nix repo

Add the `bun2nix` flake to your inputs as follows:

```nix
bun2nix.url = "github:baileyluTCD/bun2nix";
bun2nix.inputs.nixpkgs.follows = "nixpkgs";
```

## 1.5. (Optional) Use the binary cache

The bun2nix executable typically takes a while to compile, which is typical for many rust programs, hence, because of the [garnix](https://garnix.io/) based CI/CD, a convenient binary cache is provided.

To add it include the following in your `flake.nix`.

```nix
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
```

## 2. Add the binary to your environment

Next, add the bun2nix program into your developer environment by adding it to your [devshell](https://fasterthanli.me/series/building-a-rust-service-with-nix/part-10).

```nix
devShells.default = pkgs.mkShell {
  packages = with pkgs; [
    bun
    bun2nix.packages.${system}.default
  ];
};
```

> NOTE: the `system` variable can be gotten in a variety of convenient ways - including [flake-utils](https://github.com/numtide/flake-utils) or [nix-systems](https://github.com/nix-systems/nix-systems).

## 3. Use the binary in a bun `postinstall` script

To keep the generated `bun.nix` file produced by bun2nix up to date, add `bun2nix` as a postinstall script to run it after every bun operation that modifies the packages in some way.

Add the following to `package.json`:

```json
"scripts": {
    "postinstall": "bun2nix -o bun.nix"
}
```

## 4. Build your package with nix

Finally, a convenient package builder is exposed inside `bun2nix` - `mkBunDerivation`.

Add the following to `flake.nix`:

```nix
my-package = pkgs.callPackage ./default.nix {
    inherit (bun2nix.lib.${system}) mkBunDerivation;
};
```

And place this in a file called `default.nix`

```nix
{ mkBunDerivation, ... }:
mkBunDerivation {
  name = "bun2nix-example";
  version = "1.0.0";

  src = ./.;

  bunNix = ./bun.nix;

  index = ./index.ts;
}
```

A list of available options for `mkBunDerivation` can be seen at [the building packages page](./building-packages.md).
