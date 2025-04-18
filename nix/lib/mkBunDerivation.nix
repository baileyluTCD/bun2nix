{
  bun,
  callPackage,
  lib,
  rsync,
  stdenv,
  ...
}:
{
  pname,
  version,
  src,
  bunNix,
  ...
}@args:
let
  bunDeps = callPackage bunNix { };
in
stdenv.mkDerivation (
  {
    inherit pname version src;

    nativeBuildInputs = [
      rsync
      bun
    ];

    phases = [
      "unpackPhase"
      "loadModulesPhase"
      "buildPhase"
      "installPhase"
    ];

    # Load node_modules based on the expression generated from the lockfile
    loadModulesPhase = ''
      runHook preLoadModules

      # Preserve symlinks in .bin
      rsync -a --copy-links --chmod=ugo+w --exclude=".bin" ${bunDeps.nodeModules}/node_modules/ ./node_modules/

      if [ -d "${bunDeps.nodeModules}/node_modules/.bin" ]; then
        rsync -a --links ${bunDeps.nodeModules}/node_modules/.bin/ ./node_modules/.bin/
      fi

      mkdir tmp
      export HOME=$TMPDIR

      runHook postLoadModules
    '';

    # Create a react static html site as per the script
    buildPhase =
      assert lib.assertMsg (args.index != null)
        "`index` input to `mkBunDerivation` pointing to your javascript index file must be set in order to use the default buildPhase";
      ''
        runHook preBuild

        # Create a bun binary with all the highest compile time optimizations enabled
        bun build \
          --compile \
          --minify \
          --sourcemap \
          --bytecode \
          ${args.index} \
          --outfile ${pname}

        runHook postBuild
      '';

    # Install the binary to the output folder
    installPhase = ''
      runHook preInstall

      mkdir -p $out/bin

      cp ./${pname} $out/bin

      runHook postInstall
    '';

    # Bun binaries are broken by fixup phase
    dontFixup = true;
  }
  // lib.optionalAttrs (args.buildPhase == null && args.installPhase == null) {
    meta.mainProgram = pname;
  }
  // args
)
