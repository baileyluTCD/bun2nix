//! Deterministic Nix-attrset rendering for registry [`VersionMeta`].
//!
//! Rendering the nested maps/lists of a manifest entry as a Nix attribute set
//! is awkward in a template, so it is done here in Rust where quoting, string
//! escaping and ordering can be controlled precisely. The result is spliced
//! into the output template verbatim (the template escaper is a no-op).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use bun2nix_core::manifest::meta::VersionMeta;

/// Escape a string so it is a valid Nix double-quoted string body.
///
/// * `\` → `\\`
/// * `"` → `\"`
/// * `${` → `\${` (otherwise Nix would treat it as antiquotation)
fn escape_nix_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' if chars.peek() == Some(&'{') => {
                // Escape the antiquotation opener so `${` is literal.
                out.push_str("\\${");
                chars.next();
            }
            other => out.push(other),
        }
    }
    out
}

/// Render a quoted, escaped Nix string literal (`"..."`).
fn nix_string(s: &str) -> String {
    format!("\"{}\"", escape_nix_string(s))
}

/// Render a `BTreeMap<String, String>` as a Nix attribute set at the given
/// indentation. Empty maps render as `{ }`; keys and values are both escaped.
fn render_str_map(out: &mut String, map: &BTreeMap<String, String>, indent: usize) {
    if map.is_empty() {
        out.push_str("{ }");
        return;
    }

    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);
    out.push_str("{\n");
    for (k, v) in map {
        let _ = writeln!(out, "{inner}{} = {};", nix_string(k), nix_string(v));
    }
    let _ = write!(out, "{pad}}}");
}

/// Render a `&[String]` as a Nix list. Empty lists render as `[ ]`; otherwise
/// the elements are space-separated escaped strings (`[ "a" "b" ]`).
fn render_str_list(out: &mut String, list: &[String]) {
    if list.is_empty() {
        out.push_str("[ ]");
        return;
    }

    out.push_str("[ ");
    for s in list {
        out.push_str(&nix_string(s));
        out.push(' ');
    }
    out.push(']');
}

/// Render a [`VersionMeta`] as a Nix attribute set at the given indentation
/// (the column at which the opening `{` sits, used to align nested entries).
/// `registry` (when `Some`) is emitted first as a `registry` attr — the
/// non-default registry href keying this entry's `.npm` manifest.
///
/// Note: the `integrity` field is deliberately **not** emitted — it is rebuilt
/// downstream from the entry `hash`.
pub fn render_version_meta(
    out: &mut String,
    meta: &VersionMeta,
    registry: Option<&str>,
    indent: usize,
) {
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);

    out.push_str("{\n");

    if let Some(href) = registry {
        let _ = writeln!(out, "{inner}registry = {};", nix_string(href));
    }

    let _ = writeln!(
        out,
        "{inner}tarballUrl = {};",
        nix_string(&meta.tarball_url)
    );

    let _ = write!(out, "{inner}dependencies = ");
    render_str_map(out, &meta.dependencies, indent + 2);
    out.push_str(";\n");

    let _ = write!(out, "{inner}peerDependencies = ");
    render_str_map(out, &meta.peer_dependencies, indent + 2);
    out.push_str(";\n");

    let _ = write!(out, "{inner}optionalDependencies = ");
    render_str_map(out, &meta.optional_dependencies, indent + 2);
    out.push_str(";\n");

    let _ = write!(out, "{inner}optionalPeers = ");
    render_str_list(out, &meta.optional_peers);
    out.push_str(";\n");

    let _ = write!(out, "{inner}bin = ");
    render_str_map(out, &meta.bin, indent + 2);
    out.push_str(";\n");

    let _ = write!(out, "{inner}os = ");
    render_str_list(out, &meta.os);
    out.push_str(";\n");

    let _ = write!(out, "{inner}cpu = ");
    render_str_list(out, &meta.cpu);
    out.push_str(";\n");

    let _ = writeln!(
        out,
        "{inner}hasInstallScript = {};",
        meta.has_install_script
    );

    let _ = write!(out, "{pad}}}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_backslash_quote_and_antiquotation() {
        assert_eq!(
            escape_nix_string("back\\slash \"q\" ${x}"),
            "back\\\\slash \\\"q\\\" \\${x}"
        );
    }

    #[test]
    fn empty_collections_render_compactly() {
        let mut out = String::new();
        render_str_map(&mut out, &BTreeMap::new(), 0);
        assert_eq!(out, "{ }");

        let mut out = String::new();
        render_str_list(&mut out, &[]);
        assert_eq!(out, "[ ]");
    }

    #[test]
    fn list_is_space_separated() {
        let mut out = String::new();
        render_str_list(&mut out, &["linux".to_string(), "darwin".to_string()]);
        assert_eq!(out, "[ \"linux\" \"darwin\" ]");
    }
}
