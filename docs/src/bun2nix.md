# Bun2Nix

<div style="display: flex; flex-direction: column; align-items: center; width: 100%;">
    <img src="./favicon.svg" alt="Bun2nix Logo" width="150" height="150">
</div>

Bun2nix is a fast rust based tool for converting lockfiles generated with the [JavaScript](https://en.wikipedia.org/wiki/JavaScript) [Bun](https://bun.sh/) (v1.2+) package manager files to [Nix](https://nixos.wiki/) expressions, which allows them to be consumed to build Bun packages reproducibly.

## Advantages

Here are some of the advantages of using bun/bun2nix over the alternatives:

- Much faster than other similar lang2nix tools for the javascript ecosystem - a full cached install will only take around 50ms for a medium project with 2k packages
- Build aot complied binaries easily with bun that fit the nix model much better than npm scripts
- Quality error messages because of the static types in rust

## Alternatives

Here are some alternatives to bun2nix in the JavaScript ecosystem which fulfill a similar purpose:

- [node2nix](https://github.com/svanderburg/node2nix)
- [yarn2nix](https://github.com/nix-community/yarn2nix)
- [js2nix](https://github.com/canva-public/js2nix)
