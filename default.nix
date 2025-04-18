{
  lib,
  rustPlatform,
  pkg-config,
  openssl,
  stdenv,
  bun,
  callPackage,
  rsync,
}:
let
  cargoTOML = builtins.fromTOML (builtins.readFile ./Cargo.toml);
in
{
  # Bun2nix binary
  bin = rustPlatform.buildRustPackage {
    pname = cargoTOML.package.name;
    version = cargoTOML.package.version;

    src = ./.;

    nativeBuildInputs = [
      pkg-config
      openssl
    ];

    buildInputs = [
      pkg-config
      openssl
    ];

    # Disable network using tests
    checkFlags = [
      "--skip=test_parse_minimal_lockfile"
      "--skip=test_parse_react_lockfile"
      "--skip=test_parse_bunx_binary_lockfile"
      "--skip=test_prefetch_packages"
    ];

    cargoLock = {
      lockFile = ./Cargo.lock;
    };

    meta = with lib; {
      description = "A fast rust based bun lockfile to nix expression converter.";
      homepage = "https://github.com/baileyluTCD/bun2nix";
      license = licenses.mit;
      maintainers = [ "baileylu@tcd.ie" ];
    };
  };

  # Custom builder function for `bun2nix` packages
  mkBunDerivation =
    {
      name,
      version,
      src,
      bunNix,
      buildFlags ? [
        "--compile"
        "--minify"
        "--sourcemap"
        "--bytecode"
      ],
      ...
    }@args:
    let
      bunDeps = callPackage bunNix { };
    in
    stdenv.mkDerivation (
      {
        inherit name version src;

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
            bun build ${lib.concatStringsSep " " buildFlags} ${args.index} --outfile ${name}

              runHook postBuild
          '';

        # Install the binary to the output folder
        installPhase = ''
          runHook preInstall

          mkdir -p $out/bin

          cp ./${name} $out/bin

          runHook postInstall
        '';

        # Bun binaries are broken by fixup phase
        dontFixup = true;
      }
      // args
    );
}
