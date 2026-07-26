# Regression guard for issue #71 (non-default registries): offline
# peer-dependency resolution for packages resolved from a non-default npm
# registry (https://registry.npmmirror.com, pinned via the fixture's committed
# bunfig.toml).
#
# bun.lock is EXCLUDED from the source, so bun must resolve peer deps at
# install time from the synthesized manifest cache.  For a non-default
# registry bun looks up `<wyhash(name)>-<wyhash(registry_href)>.npm`, deriving
# the registry href from ./bunfig.toml inside the sandbox — so a green build
# proves the generation-time key (computed from the same committed config)
# matches byte-for-byte.  If the registry-keyed manifests are absent or
# mis-keyed, bun falls back to the network, which the sandbox blocks → the
# build fails.
_: {
  perSystem =
    { config, ... }:
    {
      checks.nonDefaultRegistryOfflineInstall = config.mkDerivation.function {
        packageJson = ./non-default-registry-offline-install/fixture/package.json;

        src = builtins.path {
          path = ./non-default-registry-offline-install/fixture;
          name = "non-default-registry-offline-install-fixture-src";
          # Exclude node_modules (working-tree artefact) and bun.lock (forces
          # manifest-cache resolution).  bunfig.toml MUST stay included — bun
          # derives the manifest-cache registry key from it in the sandbox.
          filter =
            path: _type:
            let
              base = builtins.baseNameOf path;
            in
            base != "node_modules" && base != "bun.lock";
        };

        bunDeps = config.fetchBunDeps.function {
          bunNix = ./non-default-registry-offline-install/fixture/bun.nix;
        };

        # Skip lifecycle scripts and bun build — we only care that
        # `bun install` resolves deps offline via the registry-keyed cache.
        dontRunLifecycleScripts = true;

        buildPhase = ''
          echo "bun install resolved non-default-registry deps offline — manifest cache working"
        '';

        installPhase = ''
          mkdir -p "$out"
          echo "nonDefaultRegistryOfflineInstall: PASS" > "$out/result"
        '';
      };
    };
}
