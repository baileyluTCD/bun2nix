# Bun2Nix sample

Example project created with `bun init` to show usage of `bun2nix`, a fast bun package to nix expression converter written in rust.

This project uses a binary that would be typically installed via `bunx example-binary` in the build phase.

For the purposes of this example, [cowsay](https://www.npmjs.com/package/cowsay) is ran before the `buildPhase`.

To try it out use `nix run .`.

## Notable files

The main files of note are:
- `flake.nix` -> Contains basic project setup for a nix flake for `bun2nix`
- `default.nix` -> Contains build instructions for this bun package
- `bun.nix` -> Generated bun expression from `bun.lock`
- `package.json` -> Standard Javascript `package.json` with a `postinstall` script pointing to `bun2nix`
