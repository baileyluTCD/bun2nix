{
  flake,
  system,
  ...
}:
flake.lib.${system}.treefmt.config.build.check flake
