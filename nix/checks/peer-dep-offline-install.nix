# Regression guard for issue #71: peer-dependency resolution offline.
#
# react-dom@19 peer-depends on react.  Before the manifest-cache fix, bun had
# no offline source for peer-dep manifests and fell back to the network even
# when all tarballs were cached, producing:
#   error: ConnectionRefused downloading package manifest
#
# The guard is non-vacuous because bun.lock is EXCLUDED from the source (see
# filter below).  Without a lockfile bun must resolve peer deps from scratch;
# it uses the synthesised .npm manifest files (from `manifest = {}` attrs in
# bun.nix, written to BUN_INSTALL_CACHE_DIR by fetchBunDeps).  If those files
# are absent bun falls back to the network, which is blocked in the Nix
# sandbox → build fails.  With them it succeeds in ~1 ms.
#
# Evidence: run `nix build` with bun.nix manifest attrs stripped (or with
# BUN_MANIFEST_CACHE unset) and observe:
#   error: ... ConnectionRefused downloading package manifest
_: {
  perSystem =
    { config, ... }:
    {
      checks.peerDepOfflineInstall = config.mkDerivation.function {
        packageJson = ./peer-dep-offline-install/fixture/package.json;

        src = builtins.path {
          path = ./peer-dep-offline-install/fixture;
          name = "peer-dep-offline-install-fixture-src";
          # Exclude node_modules (working-tree artefact) and bun.lock.
          # Omitting bun.lock forces bun to resolve peer deps at install time
          # using the synthesised manifest cache — that is the code path that
          # failed in issue #71 and is guarded here.
          filter =
            path: _type:
            let
              base = builtins.baseNameOf path;
            in
            base != "node_modules" && base != "bun.lock";
        };

        bunDeps = config.fetchBunDeps.function {
          bunNix = ./peer-dep-offline-install/fixture/bun.nix;
        };

        # Skip lifecycle scripts and bun build — we only care that
        # `bun install` resolves peer deps offline via the manifest cache.
        dontRunLifecycleScripts = true;

        buildPhase = ''
          echo "bun install resolved peer deps offline — manifest cache working"
        '';

        installPhase = ''
          mkdir -p "$out"
          echo "peerDepOfflineInstall: PASS" > "$out/result"
        '';
      };
    };
}
