{ ... }:
{
  projectRootFile = "flake.nix";

  programs.deadnix.enable = true;
  programs.nixfmt.enable = true;
  programs.shfmt.enable = true;
  programs.rustfmt.enable = true;
  programs.prettier.enable = true;
  programs.toml-sort.enable = true;
  programs.sql-formatter.enable = true;
}
