use serde::{Deserialize, Serialize};
use async_process::Command;
use crate::{error::Error, Result};

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

#[derive(Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorePrefetch {
    pub hash: String,
    pub store_path: String
}

impl PrefetchedPackage {
    /// # Prefetch Package
    ///
    /// Prefetch a package from a url and produce a `PrefetchedPackage`
    pub async fn prefetch(name: String, url: String) -> Result<Self> {
        let output = Command::new("nix")
            .args([
                "store",
                "prefetch-file",
                "--json",
                &url,
            ])
            .output()
            .await?;

        if !output.status.success() {
            return Err(Error::PrefetchStdError(String::from_utf8(output.stderr)?));
        }

        let store_return: StorePrefetch = serde_json::from_slice(&output.stdout)?;

        Ok(Self{
            name,
            url,
            hash: store_return.hash
        })
    }

    fn get_name_strip_version(&self) -> Result<&str> {
        match self.name.matches("@").count() {
            1 => Ok(self.name.split_once('@').ok_or(Error::NoAtInPackageIdentifier)?.0),
            2 => self.name.rsplitn(2, '@').last().ok_or(Error::NoAtInPackageIdentifier),
            _ => Err(Error::NoAtInPackageIdentifier)
        }
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
      name = \"{}\";
      path = fetchurl {{
        name = \"{}\";
        url  = \"{}\";
        hash = \"{}\";
      }};
    }}",
            self.get_name_strip_version().unwrap_or(&self.name), self.name, self.url, self.hash
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
{{
  fetchurl,
  gnutar,
  coreutils,
  runCommand,
  symlinkJoin,
}}: let
  # Bun packages to install
  packages = [
{}
  ];

  # Extract a package from a tar file
  extractPackage = pkg:
    runCommand \"bun2nix-extract-${{pkg.name}}\" {{buildInputs = [gnutar coreutils];}} ''
      mkdir -p $out/${{pkg.name}}
      tar -xzf ${{pkg.path}} -C $out/${{pkg.name}} --strip-components=1
    '';

  # Build the node modules directory
  nodeModules = symlinkJoin {{
    name = \"node-modules\";
    paths = map extractPackage packages;
  }};
in {{
  inherit nodeModules packages;
}}",
    packages_section)
    }
}

#[test]
fn test_get_name_strip_version() {
    let a = PrefetchedPackage {
        name: "quick-lru@5.2.0".to_owned(),
        ..Default::default()
    };

    assert_eq!(a.get_name_strip_version().unwrap(), "quick-lru");

    let b = PrefetchedPackage {
        name: "@alloc/quick-lru@5.2.0".to_owned(),
        ..Default::default()
    };

    assert_eq!(b.get_name_strip_version().unwrap(), "@alloc/quick-lru");
}

#[test]
fn test_dump_nix_expression_file_single() {
    let output = PrefetchedPackage {
        hash: "sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=".to_owned(),
        url: "https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz".to_owned(),
        name: "@alloc/quick-lru@5.2.0".to_owned()
    };

    let expected = 
"    {
      name = \"@alloc/quick-lru\";
      path = fetchurl {
        name = \"@alloc/quick-lru@5.2.0\";
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
            name: "@alloc/quick-lru@5.2.0".to_owned()
        },
        PrefetchedPackage {
            hash: "sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=".to_owned(),
            url: "https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz".to_owned(),
            name: "@alloc/quick-lru@5.2.0".to_owned()
        }
    ];

    let expected = 
"# This file was autogenerated by `bun2nix`, editing it is not recommended.
# Consume it with `callPackage` in your actual derivation -> https://nixos-and-flakes.thiscute.world/nixpkgs/callpackage
{
  fetchurl,
  gnutar,
  coreutils,
  runCommand,
  symlinkJoin,
}: let
  # Bun packages to install
  packages = [
    {
      name = \"@alloc/quick-lru\";
      path = fetchurl {
        name = \"@alloc/quick-lru@5.2.0\";
        url  = \"https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz\";
        hash = \"sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=\";
      };
    }
    {
      name = \"@alloc/quick-lru\";
      path = fetchurl {
        name = \"@alloc/quick-lru@5.2.0\";
        url  = \"https://registry.npmjs.org/@alloc/quick-lru/-/quick-lru-5.2.0.tgz\";
        hash = \"sha256-w/Huz4+crTzdiSyQVAx0h3lhtTTrtPyKp3xpQD5EG9g=\";
      };
    }
  ];

  # Extract a package from a tar file
  extractPackage = pkg:
    runCommand \"bun2nix-extract-${pkg.name}\" {buildInputs = [gnutar coreutils];} ''
      mkdir -p $out/${pkg.name}
      tar -xzf ${pkg.path} -C $out/${pkg.name} --strip-components=1
    '';

  # Build the node modules directory
  nodeModules = symlinkJoin {
    name = \"node-modules\";
    paths = map extractPackage packages;
  };
in {
  inherit nodeModules packages;
}";

    assert_eq!(expected.trim(), out.dump_nix_expression().trim());
}
