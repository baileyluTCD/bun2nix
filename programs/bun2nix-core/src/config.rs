//! Project-local bun registry configuration: parse `bunfig.toml` and `.npmrc`
//! *contents* into a [`RegistryConfig`] and resolve each package's registry the
//! way bun's `scope_for_package_name` does at install time. No filesystem
//! access — callers read the files and pass their contents.

use std::collections::BTreeMap;
use std::fmt;

use url::Url;

/// The default registry href, normalized (no trailing slash).
pub const DEFAULT_REGISTRY_HREF: &str = "https://registry.npmjs.org";

/// Registry configuration merged from `bunfig.toml` and `.npmrc`
/// (`.npmrc` overrides per key). All hrefs are normalized via
/// [`normalize_href`].
#[derive(Debug, Default, Clone)]
pub struct RegistryConfig {
    default_href: Option<String>,
    /// Scope name (without the leading `@`) → registry href.
    ///
    /// bun keys its scope map by `wyhash11(scope)` with a stored-name equality
    /// guard on lookup; a string-keyed map has identical observable semantics.
    scopes: BTreeMap<String, String>,
}

/// Errors from parsing registry configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// `bunfig.toml` content was not valid TOML.
    BunfigParse(String),
    /// A configured registry URL failed WHATWG parsing.
    InvalidRegistryUrl(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::BunfigParse(e) => write!(f, "invalid bunfig.toml: {e}"),
            ConfigError::InvalidRegistryUrl(u) => write!(f, "invalid registry URL: {u}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// WHATWG-parse a registry URL, strip credentials, and strip trailing
/// slashes/backslashes (the exact bytes bun hashes for the `.npm` key).
pub fn normalize_href(raw: &str) -> Result<String, ConfigError> {
    let mut url = Url::parse(raw).map_err(|_| ConfigError::InvalidRegistryUrl(raw.to_string()))?;
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Ok(crate::manifest::without_trailing_slash(url.as_str()).to_string())
}

/// A bunfig registry value is either a bare URL string or a table with a
/// `url` key (plus auth fields we ignore).
fn registry_value_href(value: &toml::Value) -> Option<&str> {
    match value {
        toml::Value::String(s) => Some(s),
        toml::Value::Table(t) => t.get("url").and_then(toml::Value::as_str),
        _ => None,
    }
}

fn parse_bunfig(content: &str, cfg: &mut RegistryConfig) -> Result<(), ConfigError> {
    let value: toml::Value =
        toml::from_str(content).map_err(|e| ConfigError::BunfigParse(e.to_string()))?;
    let install = value.get("install");

    if let Some(href) = install
        .and_then(|i| i.get("registry"))
        .and_then(registry_value_href)
    {
        cfg.default_href = Some(normalize_href(href)?);
    }

    if let Some(scopes) = install
        .and_then(|i| i.get("scopes"))
        .and_then(toml::Value::as_table)
    {
        for (key, value) in scopes {
            if let Some(href) = registry_value_href(value) {
                cfg.scopes.insert(
                    key.trim_start_matches('@').to_string(),
                    normalize_href(href)?,
                );
            }
        }
    }
    Ok(())
}

fn parse_npmrc(content: &str, cfg: &mut RegistryConfig) -> Result<(), ConfigError> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty() {
            continue;
        }
        if key == "registry" {
            cfg.default_href = Some(normalize_href(value)?);
        } else if let Some(scope) = key
            .strip_prefix('@')
            .and_then(|k| k.strip_suffix(":registry"))
        {
            cfg.scopes
                .insert(scope.trim().to_string(), normalize_href(value)?);
        }
    }
    Ok(())
}

impl RegistryConfig {
    /// Merge `bunfig.toml` then `.npmrc` contents (each optional). `.npmrc`
    /// is applied second, so it overrides bunfig per key — default registry
    /// and each scope independently.
    pub fn parse(bunfig: Option<&str>, npmrc: Option<&str>) -> Result<Self, ConfigError> {
        let mut cfg = RegistryConfig::default();
        if let Some(content) = bunfig {
            parse_bunfig(content, &mut cfg)?;
        }
        if let Some(content) = npmrc {
            parse_npmrc(content, &mut cfg)?;
        }
        Ok(cfg)
    }

    /// The registry a package resolves to under this config, mirroring bun's
    /// `scope_for_package_name`: scoped packages consult the per-scope map
    /// first, everything else (including scope misses) uses the default
    /// registry. Returns `None` when the result is the npmjs default.
    pub fn scope_for_package_name(&self, package_name: &str) -> Option<&str> {
        let href = if let Some(rest) = package_name.strip_prefix('@') {
            let scope = rest.split('/').next().unwrap_or(rest);
            self.scopes
                .get(scope)
                .map(String::as_str)
                .or(self.default_href.as_deref())
        } else {
            self.default_href.as_deref()
        };
        href.filter(|h| *h != DEFAULT_REGISTRY_HREF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_config_means_default_registry() {
        let cfg = RegistryConfig::parse(None, None).unwrap();
        assert_eq!(cfg.scope_for_package_name("react"), None);
        assert_eq!(cfg.scope_for_package_name("@types/node"), None);
    }

    #[test]
    fn bunfig_default_registry_string_form() {
        let bunfig = "[install]\nregistry = \"https://registry.npmmirror.com/\"\n";
        let cfg = RegistryConfig::parse(Some(bunfig), None).unwrap();
        assert_eq!(
            cfg.scope_for_package_name("react"),
            Some("https://registry.npmmirror.com")
        );
    }

    #[test]
    fn bunfig_object_form_and_scopes() {
        let bunfig = r#"
[install]
registry = { url = "https://registry.npmmirror.com", token = "secret" }

[install.scopes]
"@myorg" = "https://npm.example.com/"
other = { url = "https://other.example.com" }
"#;
        let cfg = RegistryConfig::parse(Some(bunfig), None).unwrap();
        assert_eq!(
            cfg.scope_for_package_name("@myorg/pkg"),
            Some("https://npm.example.com")
        );
        // Scope keys work with or without the leading '@'.
        assert_eq!(
            cfg.scope_for_package_name("@other/pkg"),
            Some("https://other.example.com")
        );
        // Unknown scope falls back to the default registry.
        assert_eq!(
            cfg.scope_for_package_name("@unknown/pkg"),
            Some("https://registry.npmmirror.com")
        );
    }

    #[test]
    fn npmrc_overrides_bunfig_per_key() {
        let bunfig = "[install]\nregistry = \"https://a.example.com\"\n\n[install.scopes]\n\"@s\" = \"https://b.example.com\"\n";
        let npmrc = "registry=https://c.example.com/\n# comment\n; also a comment\n@s:registry=https://d.example.com/\n";
        let cfg = RegistryConfig::parse(Some(bunfig), Some(npmrc)).unwrap();
        assert_eq!(
            cfg.scope_for_package_name("x"),
            Some("https://c.example.com")
        );
        assert_eq!(
            cfg.scope_for_package_name("@s/x"),
            Some("https://d.example.com")
        );
    }

    #[test]
    fn npmjs_href_resolves_to_default() {
        let npmrc = "registry=https://registry.npmjs.org/\n";
        let cfg = RegistryConfig::parse(None, Some(npmrc)).unwrap();
        assert_eq!(cfg.scope_for_package_name("react"), None);
    }

    #[test]
    fn credentials_are_stripped_from_href() {
        let npmrc = "registry=https://user:pass@npm.example.com/\n";
        let cfg = RegistryConfig::parse(None, Some(npmrc)).unwrap();
        assert_eq!(
            cfg.scope_for_package_name("react"),
            Some("https://npm.example.com")
        );
    }

    #[test]
    fn path_hrefs_keep_their_path() {
        let npmrc = "registry=https://example.com/npm/\n";
        let cfg = RegistryConfig::parse(None, Some(npmrc)).unwrap();
        assert_eq!(
            cfg.scope_for_package_name("react"),
            Some("https://example.com/npm")
        );
    }

    #[test]
    fn invalid_registry_url_is_an_error() {
        assert!(RegistryConfig::parse(None, Some("registry=not a url\n")).is_err());
        assert!(RegistryConfig::parse(Some("[install]\nregistry = \"::nope::\"\n"), None).is_err());
    }

    #[test]
    fn default_href_matches_manifest_registry_url() {
        assert_eq!(
            normalize_href(crate::manifest::DEFAULT_REGISTRY_URL).unwrap(),
            DEFAULT_REGISTRY_HREF
        );
    }
}
