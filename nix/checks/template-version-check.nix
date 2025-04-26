{
  pkgs,
  ...
}:
let
  cargoToml = builtins.fromTOML (builtins.readFile ../../Cargo.toml);

  currentVersion = cargoToml.package.version;

  templatesDir = ../templates;
in
pkgs.stdenv.mkDerivation {
  name = "template-versions-check";

  dontBuild = true;

  src = ./.;

  doCheck = true;

  checkPhase = ''
    echo "Checking template tag versions match current cargo toml version..."
    templates=$(ls ${templatesDir})

    for template in $templates; do
      echo "Checking '$template' template version..."

      version=$(
        cat "${templatesDir}/$template/flake.nix" | \
        grep -Po 'bun2nix\.url = "github:baileyluTCD/bun2nix\?tag=\K[0-9]+\.[0-9]+\.[0-9]+'
      )

      if ! [[ $version == ${currentVersion} ]]; then
        echo "Tag version $version does not match ${currentVersion} for template '$template'."
        exit 1
      fi

    done
  '';

  installPhase = ''
    mkdir "$out"
  '';
}
