# Workspace template for bun2nix
{
  mkBunDerivation,
}:

mkBunDerivation {
  pname = "workspace-test-app";
  version = "1.0.0";

  src = ./packages/app;
  bunNix = ./bun.nix;

  # Points to the workspace directory
  workspaceRoot = ./.;

  # This would be the alternative approach using explicit mappings:
  # workspaces = {
  #   "@workspace/lib" = ./packages/lib;
  # };

  # Since we're not using the default build approach
  buildPhase = ''
    echo "Building workspace app..."
    echo "Checking node_modules structure:"
    ls -la node_modules/@workspace || true

    # Check if workspace lib is linked
    ls -la node_modules/@workspace/lib || true

    bun run build
  '';

  installPhase = ''
    mkdir -p $out/bin

    cp dist/app $out/bin/workspace-test-app;

    chmod +x $out/bin/workspace-test-app
  '';
}
