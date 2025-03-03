use base64::{engine::general_purpose, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::Result;

#[derive(Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// # Prefetched Package
///
/// A model of the results returned by `nix-flake-prefetch <url>`
pub struct PrefetchedPackage {
    /// The prefetched hash of the package
    pub hash: String,
    /// The url to fetch the package from
    pub url: String,
    /// The name of the package in npm
    pub name: String,
}

impl PrefetchedPackage {
    /// # Prefetch Package
    ///
    /// Prefetch a package from a url and produce a `PrefetchedPackage`
    pub async fn prefetch(name: String, url: String) -> Result<Self> {
        let response = reqwest::get(&url)
            .await?
            .bytes()
            .await?;

        let mut hasher = Sha256::new();
        hasher.update(&response);
        let hash_bytes = hasher.finalize();

        let base64 = general_purpose::STANDARD.encode(hash_bytes);
        let hash = format!("sha256-{}", base64);

        assert_eq!(51, hash.len(), "hash was not 51 chars: {}", hash);
        assert!(hash.contains("sha256"));

        Ok(Self{
            name,
            url,
            hash
        })
    }
}

/// # Nix Expression Conversion Trait
///
/// Implemented by anything that can be turned into a nix expression
pub trait DumpNixExpression {
    /// # Dump Nix Experession
    ///
    /// Dumps `self` into a nix expression
    fn dump_nix_expression(&self) -> String;
}

impl DumpNixExpression for PrefetchedPackage {
    fn dump_nix_expression(&self) -> String {
        assert_eq!(51, self.hash.len(), "hash was not 51 chars: {}", self.hash);
        assert!(self.hash.contains("sha256"));

        format!(
"    {{
        name = \"bun-package-{}\";
        path = fetchurl {{
            name = \"{}\";
            url  = \"{}\";
            hash = \"{}\";
        }};
    }}",
            self.name, self.name, self.url, self.hash
        )
    }
}

impl DumpNixExpression for Vec<PrefetchedPackage> {
    fn dump_nix_expression(&self) -> String {
        let packages_section = self
            .iter()
            .map(|p| p.dump_nix_expression())
            .reduce(|acc, e| acc + "\n" + &e)
            .unwrap_or_default();

        format!(
"# This file was autogenerated by `bun2nix`, editing it is not recommended.
# Consume it with `callPackage` in your actual derivation -> https://nixos-and-flakes.thiscute.world/nixpkgs/callpackage
{{ fetchurl, gnutar, coreutils, runCommand }}: rec {{
  packages = [
{}
  ];
  nodeModules = runCommand \"bun-node-modules\" {{ buildInputs = [ gnutar coreutils ]; }} ''
    mkdir -p $out/node_modules

    echo \"Extracting node modules...\"
    for package in ${{builtins.concatStringsSep \" \" packages}}; do
      echo \"Extracting $package...\"
      tar -xzf \"$package\" -C $out/node_modules --strip-components=1
    done
    echo \"Node modules extracted!\"
  '';
}}",
    packages_section)
    }
}

#[test]
fn test_dump_nix_expression_file_single() {
    let output = PrefetchedPackage {
        hash: "sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=".to_owned(),
        url: "https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz".to_owned(),
        name: "@alloc/quick-lru".to_owned()
    };

    let expected = 
"    {
        name = \"bun-package-@alloc/quick-lru\";
        path = fetchurl {
            name = \"@alloc/quick-lru\";
            url  = \"https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz\";
            hash = \"sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=\";
        };
    }";

    assert_eq!(expected.trim(), output.dump_nix_expression().trim());
}

#[test]
fn test_dump_nix_expression_file_vec() {
    let out = vec![
        PrefetchedPackage {
            hash: "sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=".to_owned(),
            url: "https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz".to_owned(),
            name: "@alloc/quick-lru".to_owned()
        },
        PrefetchedPackage {
            hash: "sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=".to_owned(),
            url: "https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz".to_owned(),
            name: "@alloc/quick-lru".to_owned()
        }
    ];

    let expected = 
"# This file was autogenerated by `bun2nix`, editing it is not recommended.
# Consume it with `callPackage` in your actual derivation -> https://nixos-and-flakes.thiscute.world/nixpkgs/callpackage
{ fetchurl, gnutar, coreutils, runCommand }: rec {
  packages = [
    {
        name = \"bun-package-@alloc/quick-lru\";
        path = fetchurl {
            name = \"@alloc/quick-lru\";
            url  = \"https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz\";
            hash = \"sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=\";
        };
    }
    {
        name = \"bun-package-@alloc/quick-lru\";
        path = fetchurl {
            name = \"@alloc/quick-lru\";
            url  = \"https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz\";
            hash = \"sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=\";
        };
    }
  ];
  nodeModules = runCommand \"bun-node-modules\" { buildInputs = [ gnutar coreutils ]; } ''
    mkdir -p $out/node_modules

    echo \"Extracting node modules...\"
    for package in ${builtins.concatStringsSep \" \" packages}; do
      echo \"Extracting $package...\"
      tar -xzf \"$package\" -C $out/node_modules --strip-components=1
    done
    echo \"Node modules extracted!\"
  '';
}";

    assert_eq!(expected.trim(), out.dump_nix_expression().trim());
}
