use std::fmt;

use serde::de::{self, MapAccess, Visitor};

use crate::{
    package::{Binaries, Extracted, Identifier, MetaData},
    Package,
};

/// # Package Visitor
///
/// Used for a custom serde deserialize method as the most ergonomic rust package data type does
/// not match the type in the lockfile directly
pub struct PackageVisitor;

impl<'de> Visitor<'de> for PackageVisitor {
    type Value = Vec<Package<Extracted>>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a map of package names to tuples")
    }

    fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut packages = Vec::new();

        while let Some((name, values)) = map.next_entry::<String, Vec<serde_json::Value>>()? {
            match values.len() {
                1 => Self::deserialize_workspace_package(name, values, &mut packages)?,
                2 => Self::deserialize_tarball_package(name, values, &mut packages)?,
                3 => Self::deserialize_git_package(name, values, &mut packages)?,
                4 => Self::deserialize_npm_package(name, values, &mut packages)?,
                _ => {
                    return Err(de::Error::custom(format!(
                        "Invalid package entry for {}: expected at least 4 values",
                        name
                    )));
                }
            };
        }

        Ok(packages)
    }
}

impl PackageVisitor {
    fn deserialize_tarball_package<E>(
        name: String,
        values: Vec<serde_json::Value>,
        packages: &mut Vec<Package<Extracted>>,
    ) -> Result<(), E>
    where
        E: de::Error,
    {
        let identifier = values[0]
            .as_str()
            .ok_or_else(|| de::Error::custom("Invalid tarball identifer format"))?
            .to_string();

        let meta: MetaData = serde_json::from_str(&values[1].to_string())
            .map_err(|e| de::Error::custom(format!("Invalid metadata format: {}", e)))?;

        let pkg = Package::new(
            name,
            Identifier::Tarball(identifier.to_owned()),
            None,
            meta.binaries,
        );

        packages.push(pkg);

        Ok(())
    }

    fn deserialize_workspace_package<E>(
        name: String,
        values: Vec<serde_json::Value>,
        packages: &mut Vec<Package<Extracted>>,
    ) -> Result<(), E>
    where
        E: de::Error,
    {
        let identifier = values[0]
            .as_str()
            .ok_or_else(|| de::Error::custom("Invalid workspace identifer format"))?
            .to_string();

        assert!(
            identifier.contains("workspace:"),
            "Expected workspace package to contain `workspace:`"
        );

        let pkg = Package::new(
            name,
            Identifier::Workspace(identifier.to_owned()),
            None,
            Binaries::default(),
        );

        packages.push(pkg);

        Ok(())
    }

    fn deserialize_npm_package<E>(
        name: String,
        values: Vec<serde_json::Value>,
        packages: &mut Vec<Package<Extracted>>,
    ) -> Result<(), E>
    where
        E: de::Error,
    {
        let identifier = values[0]
            .as_str()
            .ok_or_else(|| de::Error::custom("Invalid npm identifier format"))?
            .to_string();

        let meta: MetaData = serde_json::from_str(&values[2].to_string())
            .map_err(|e| de::Error::custom(format!("Invalid metadata format: {}", e)))?;

        let hash = values[3]
            .as_str()
            .ok_or_else(|| de::Error::custom("Invalid hash format"))?
            .to_string();

        // For npm packages (not workspace packages), assert that the hash is in sri format
        assert!(
            hash.contains("sha512-"),
            "Expected hash to be in sri format and contain sha512"
        );

        let pkg = Package::new(name, Identifier::Npm(identifier), Some(hash), meta.binaries);

        packages.push(pkg);

        Ok(())
    }

    fn deserialize_git_package<E>(
        name: String,
        values: Vec<serde_json::Value>,
        packages: &mut Vec<Package<Extracted>>,
    ) -> Result<(), E>
    where
        E: de::Error,
    {
        let identifier = values[0]
            .as_str()
            .ok_or_else(|| de::Error::custom("Invalid git identifer format"))?;

        let meta: MetaData = serde_json::from_str(&values[1].to_string())
            .map_err(|e| de::Error::custom(format!("Invalid metadata format: {}", e)))?;

        let rev = values[2]
            .as_str()
            .ok_or_else(|| de::Error::custom("Invalid rev format"))?
            .to_string();

        // TODO: move rev and hash into identifier type
        let pkg = Package::new(
            name,
            Identifier::Git(identifier.to_owned()),
            Some(rev),
            meta.binaries,
        );
        packages.push(pkg);

        Ok(())
    }
}
