# Guards the hook's offline-install prep script (resolve-catalog.ts):
#
# 1. A bun.lock whose trustedDependencies / patchedDependencies drifted from
#    package.json must fail fast with a diagnostic naming the drifted
#    entries.  Without the check, bun silently re-resolves the affected
#    dependencies at install time and the drift surfaces as a wall of
#    ConnectionRefused errors in the sandbox with no hint of the cause.
# 2. Sections that merely list the same entries in a different order must
#    NOT trip the check.
# 3. `catalog:` references resolving to registry versions are rewritten to
#    the exact version, but references resolving to non-registry specs
#    (github:, git+, tarball URLs, file:) must be left alone — rewriting
#    them registers as a changed spec and forces the same re-resolve.
#
# The drifted fixture deliberately contains no `catalog:` refs, pinning that
# the drift check runs for plain projects too, not only catalog users.
_: {
  perSystem =
    { pkgs, ... }:
    {
      checks.lockfileDriftDetection = pkgs.stdenv.mkDerivation {
        name = "lockfile-drift-detection";

        dontUnpack = true;

        nativeBuildInputs = [ pkgs.bun ];

        buildPhase = ''
          export HOME="$TMPDIR"
          script=${../mk-derivation/resolve-catalog.ts}

          run() {
            rc=0
            bun --config=/dev/null --no-install "$script" "$1" >log.txt 2>&1 || rc=$?
          }

          expect() {
            if ! grep -qF "$1" log.txt; then
              echo "expected output to contain: $1"
              cat log.txt
              exit 1
            fi
          }

          # --- drifted: both sections disagree with package.json ---
          mkdir drifted
          cat > drifted/package.json <<'EOF'
          {
            "name": "fixture",
            "version": "1.0.0",
            "trustedDependencies": ["node-pty"],
            "patchedDependencies": { "left-pad@1.3.0": "patches/left-pad.patch" }
          }
          EOF
          cat > drifted/bun.lock <<'EOF'
          {
            "lockfileVersion": 1,
            "workspaces": { "": { "name": "fixture" } },
            "trustedDependencies": ["esbuild"],
            "packages": {}
          }
          EOF

          run drifted
          if [ "$rc" -eq 0 ]; then
            echo "drifted fixture: expected failure, got exit 0"
            cat log.txt
            exit 1
          fi
          expect "bun.lock is out of sync with package.json"
          expect "missing from bun.lock: node-pty"
          expect "only in bun.lock: esbuild"
          expect "patchedDependencies differ for: left-pad@1.3.0"
          echo "drifted fixture: failed with diagnostic, as intended"

          # --- synced: same entries, different order ---
          mkdir synced
          cat > synced/package.json <<'EOF'
          {
            "name": "fixture",
            "version": "1.0.0",
            "trustedDependencies": ["b-pkg", "a-pkg"],
            "patchedDependencies": { "x@1.0.0": "patches/x.patch" }
          }
          EOF
          cat > synced/bun.lock <<'EOF'
          {
            "lockfileVersion": 1,
            "workspaces": { "": { "name": "fixture" } },
            "trustedDependencies": ["a-pkg", "b-pkg"],
            "patchedDependencies": { "x@1.0.0": "patches/x.patch" },
            "packages": {}
          }
          EOF

          run synced
          if [ "$rc" -ne 0 ]; then
            echo "synced fixture: expected success, got exit $rc"
            cat log.txt
            exit 1
          fi
          echo "synced fixture: passed, ordering ignored"

          # --- catalog: registry ref rewritten, non-registry ref preserved ---
          mkdir catalog
          cat > catalog/package.json <<'EOF'
          {
            "name": "fixture",
            "version": "1.0.0",
            "dependencies": {
              "bar": "catalog:",
              "foo": "catalog:"
            }
          }
          EOF
          cat > catalog/bun.lock <<'EOF'
          {
            "lockfileVersion": 1,
            "workspaces": {
              "": {
                "name": "fixture",
                "dependencies": { "bar": "catalog:", "foo": "catalog:" }
              }
            },
            "catalog": {
              "bar": "^1.0.0",
              "foo": "github:user/repo#abcdef"
            },
            "packages": {
              "bar": ["bar@1.2.3", "", {}, "sha512-aaa"],
              "foo": ["foo@github:user/repo#abcdef", {}, "abcdef"]
            }
          }
          EOF

          run catalog
          if [ "$rc" -ne 0 ]; then
            echo "catalog fixture: expected success, got exit $rc"
            cat log.txt
            exit 1
          fi
          if ! grep -qF '"bar": "1.2.3"' catalog/package.json; then
            echo "catalog fixture: registry ref not rewritten to exact version"
            cat catalog/package.json
            exit 1
          fi
          if ! grep -qF '"foo": "catalog:"' catalog/package.json; then
            echo "catalog fixture: non-registry ref was rewritten; must stay catalog:"
            cat catalog/package.json
            exit 1
          fi
          echo "catalog fixture: rewrite behavior correct"
        '';

        installPhase = ''
          mkdir -p "$out"
          echo "lockfileDriftDetection: PASS" > "$out/result"
        '';
      };
    };
}
