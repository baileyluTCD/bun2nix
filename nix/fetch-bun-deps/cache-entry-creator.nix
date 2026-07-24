{ config, ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      packages.cacheEntryCreator = pkgs.rustPlatform.buildRustPackage (finalAttrs: {
        pname = "bun2nix-cache-entry-creator";
        inherit (config.cargoTOML.package) version;

        src = ../../programs;

        cargoLock = {
          lockFile = finalAttrs.src + "/Cargo.lock";
        };

        cargoBuildFlags = [
          "-p"
          "cache-entry-creator"
        ];

        cargoTestFlags = [
          "-p"
          "cache-entry-creator"
        ];

        doCheck = true;

        meta = {
          description = "Cache entry creator for bun packages";
          longDescription = ''
            Uses bun's specific `wyhash` implementation to calculate
            the correct location in which to place a cache entry for
            a given package after the tarball has been downloaded and
            extracted.
          '';
          mainProgram = "cache_entry_creator";
        };
      });
    };

}
