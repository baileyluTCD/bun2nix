# Offline Peer Dependency Resolution

Packages with peer dependencies — the Svelte ecosystem, React rendering libraries, and many others — require npm package _manifests_ at install time so bun can resolve those dependencies. Without manifests, `bun install` tries to contact the npm registry even when all tarballs are already cached, which fails inside the Nix sandbox and produces an error like:

```
error: ConnectionRefused downloading package manifest
```

Since v2.2.0, `bun2nix` synthesizes the manifest files bun needs, making peer-dependency-heavy packages build fully offline. This fixes [issue #71](https://github.com/nix-community/bun2nix/issues/71).

## At Generation Time

When you run `bun2nix` — for example via the `postinstall` hook right after `bun install` — it reconstructs each package's manifest metadata directly from `bun.lock` (no network access needed) and embeds it as a `manifest` attribute on each registry entry in the generated `bun.nix`:

```nix
"react-dom@19.2.7" = fetchurl {
  url = "https://registry.npmjs.org/react-dom/-/react-dom-19.2.7.tgz";
  hash = "sha512-...";
} // {
  manifest = {
    tarballUrl = "https://registry.npmjs.org/react-dom/-/react-dom-19.2.7.tgz";
    dependencies     = { "scheduler" = "^0.27.0"; };
    peerDependencies = { "react" = "^19.2.7"; };
    optionalDependencies = { };
    optionalPeers    = [ ];
    bin              = { };
    os               = [ ];
    cpu              = [ ];
    hasInstallScript = false;
  };
};
```

Packages resolved from a **non-default registry** — configured in a project-local `bunfig.toml` (`[install].registry`, `[install.scopes]`) or `.npmrc` (`registry=`, `@scope:registry=`) committed next to the lockfile — additionally carry a `registry` attribute inside the `manifest` block. `bun2nix` reads only those two project-local files (`.npmrc` overriding `bunfig.toml` per key): the offline `bun install` runs in the Nix sandbox where global config, environment variables, and CLI flags do not exist, so the manifest cache key must be derived from exactly the config bun will see there. A registry configured only in `~/.npmrc` or `BUN_CONFIG_REGISTRY` cannot work offline and is deliberately ignored.

## At Build Time

The Nix build is fully offline — no extra setup required.

[`fetchBunDeps`](./building-packages/fetchBunDeps.md) reads the `manifest` attributes from `bun.nix` and runs `cache-entry-creator manifest` to synthesize the binary `.npm` cache files bun looks for at resolve time. These files are merged into the dependency cache alongside the regular package tarballs.

The [install hook](./building-packages/hook.md) automatically exports `BUN_MANIFEST_CACHE=2`, which tells bun to use the on-disk manifest cache instead of hitting the network.

## Graceful Degradation

Manifests are reconstructed from `bun.lock`, not fetched, so npm-registry entries always carry a `manifest` attribute — there is no network step that can fail or get skipped. Only entries that aren't npm-registry packages (git, GitHub, tarball, workspace, and file dependencies) have no `manifest`.

Older `bun.nix` files generated before this feature, which have no `manifest` attributes at all, still build: `fetchBunDeps` synthesizes an empty manifest cache for them, matching the behavior of older releases.

Projects that do not have peer-dependency-heavy packages are unaffected.

## Limitations

- **Registry authentication is not covered.** The manifest cache key excludes credentials, so private registries key correctly, but fetching auth-gated tarballs still relies on `fetchBunDeps`'s `bunfigPath`/`npmrcPath` credential support.
- **Non-default registries must be configured in committed project-local config** (`bunfig.toml` or `.npmrc` next to `bun.lock`) — see above. Since `.npmrc` commonly holds tokens and is gitignored, prefer `bunfig.toml` for the registry URL itself.
