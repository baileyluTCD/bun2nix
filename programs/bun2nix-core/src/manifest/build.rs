//! General multi-version `.npm` manifest builder.
//!
//! [`build_manifest`] converts a [`meta::PackageMeta`] (which may contain
//! multiple versions) into a [`serialize::PackageManifest`] suitable for
//! writing with [`serialize::write`].
//!
//! The implementation reuses [`super::StringArena`] and
//! [`super::build_dep_group`] — the same interning primitives used by
//! [`super::build_single_version`] — so string-buffer logic lives in one place.

use std::cmp::Ordering;

use super::{
    StringArena, build_dep_group,
    layout::{
        Architecture, Bin, BinValue, DistTagMap, ExternVersionMap, ExternalString,
        ExternalStringList, Integrity, IntegrityTag, NpmPackage, OperatingSystem, PackageVersion,
        PackageVersionList, SemverString, SemverVersion, Tag, VersionSlice,
    },
    meta::{PackageMeta, VersionMeta},
    serialize::PackageManifest,
};

// ──────────────────────────────────────────────────────────────────────────
// Public entry point
// ──────────────────────────────────────────────────────────────────────────

/// Build an in-memory [`PackageManifest`] for **all** versions in `pkg`.
///
/// Versions are sorted deterministically (ascending semver) before being
/// written, so the resulting binary is independent of input order.
///
/// The builder only handles registry packages (name + versions with a semver
/// string and a tarball URL).  Git/tarball/workspace packages must not be
/// passed here.
///
/// `NpmPackage.public_max_age` is set to [`u32::MAX`] so bun never considers
/// the manifest stale when running in offline mode.
pub fn build_manifest(pkg: &PackageMeta) -> PackageManifest {
    let mut arena = StringArena::default();

    // Shared external-string buffers, grown monotonically so slice offsets
    // recorded in earlier versions remain stable.
    let mut names: Vec<ExternalString> = Vec::new();
    let mut values: Vec<ExternalString> = Vec::new();
    let mut bin_entries: Vec<ExternalString> = Vec::new();

    // Package name (inlined if ≤ 8 bytes, stored in the arena otherwise).
    let name_ext = arena.intern(&pkg.name);

    // Partition into releases and pre-releases: bun's `find_by_version` picks
    // the map by `version.tag.has_pre()`, so a pre-release stored under
    // `releases` is unreachable.  Both maps index contiguous slices of the one
    // shared versions buffer — releases first, then prereleases.
    let mut releases: Vec<(&VersionMeta, ParsedVersion<'_>)> = Vec::new();
    let mut prereleases: Vec<(&VersionMeta, ParsedVersion<'_>)> = Vec::new();
    for vm in &pkg.versions {
        let pv = parse_version(&vm.version);
        if pv.pre.is_empty() {
            releases.push((vm, pv));
        } else {
            prereleases.push((vm, pv));
        }
    }

    // Sort ascending: bun's `find_best_version` scans each list from the end
    // and takes the first satisfying version, assuming ascending order.
    // Pre-release tags order by bun's dot-segment rules (numeric-aware); the
    // version-string tiebreak keeps output deterministic when two tags compare
    // equal (e.g. "1" vs "01").
    releases.sort_by(|a, b| {
        triple(&a.1)
            .cmp(&triple(&b.1))
            .then_with(|| a.0.version.cmp(&b.0.version))
    });
    prereleases.sort_by(|a, b| {
        triple(&a.1)
            .cmp(&triple(&b.1))
            .then_with(|| order_pre(a.1.pre, b.1.pre))
            .then_with(|| a.0.version.cmp(&b.0.version))
    });

    let n_rel = releases.len() as u32;
    let n_pre = prereleases.len() as u32;

    let mut semver_versions: Vec<SemverVersion> = Vec::with_capacity(pkg.versions.len());
    let mut package_versions: Vec<PackageVersion> = Vec::with_capacity(pkg.versions.len());

    for (vm, parsed) in releases.iter().chain(prereleases.iter()) {
        // Empty tag components stay `ExternalString::default()` (hash 0):
        // `Tag::eql` compares pre hashes, and bun parses a tagless version
        // query to hash 0 — interning "" would store wyhash11(0, "") instead.
        let tag = Tag {
            pre: intern_tag(&mut arena, parsed.pre),
            build: intern_tag(&mut arena, parsed.build),
        };
        semver_versions.push(SemverVersion {
            major: parsed.major,
            minor: parsed.minor,
            patch: parsed.patch,
            tag,
        });

        let pv = build_one_version(vm, &mut arena, &mut names, &mut values, &mut bin_entries);
        package_versions.push(pv);
    }

    let n = n_rel + n_pre;
    let n_names = names.len() as u32;
    let external_strings = names.into_boxed_slice();
    let external_strings_for_versions = values.into_boxed_slice();
    let extern_strings_bin_entries = bin_entries.into_boxed_slice();

    let mut pkg_struct = NpmPackage {
        name: name_ext,
        // Never expire — so bun treats this manifest as fresh indefinitely
        // under BUN_MANIFEST_CACHE=2 (verified in the Task 3 spike).
        public_max_age: u32::MAX,
        // With `minimumReleaseAge` configured, bun rejects any cached manifest
        // lacking extended data (publish timestamps) and refetches it — fatal
        // offline. Our zeroed `publish_timestamp_ms` reads as "published at
        // epoch", which passes every age gate; correct for lockfile-pinned
        // versions.
        has_extended_manifest: true,
        ..NpmPackage::default()
    };

    // releases occupy [0, n_rel) of the shared buffers, prereleases
    // [n_rel, n_rel + n_pre).
    pkg_struct.releases = ExternVersionMap {
        keys: VersionSlice::new(0, n_rel),
        values: PackageVersionList::new(0, n_rel),
    };
    pkg_struct.prereleases = ExternVersionMap {
        keys: VersionSlice::new(n_rel, n_pre),
        values: PackageVersionList::new(n_rel, n_pre),
    };
    // dist_tags: left empty (v1 scope — see brief "Leave dist_tags empty").
    pkg_struct.dist_tags = DistTagMap::default();
    pkg_struct.versions_buf = VersionSlice::new(0, n);
    pkg_struct.string_lists_buf = ExternalStringList::new(0, n_names);

    PackageManifest {
        pkg: pkg_struct,
        string_buf: arena.buf.into_boxed_slice(),
        versions: semver_versions.into_boxed_slice(),
        external_strings,
        external_strings_for_versions,
        package_versions: package_versions.into_boxed_slice(),
        extern_strings_bin_entries,
        bundled_deps_buf: Box::new([]),
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Per-version helper
// ──────────────────────────────────────────────────────────────────────────

/// Build one [`PackageVersion`] from a [`VersionMeta`], appending strings to
/// the shared arenas.  This is the per-version helper called in a loop by
/// [`build_manifest`], factored out so that the interning/dep-group/integrity
/// logic is not duplicated.
fn build_one_version(
    vm: &VersionMeta,
    arena: &mut StringArena,
    names: &mut Vec<ExternalString>,
    values: &mut Vec<ExternalString>,
    bin_entries: &mut Vec<ExternalString>,
) -> PackageVersion {
    // Dependency groups (three groups: deps / optional / peer).
    let dependencies = build_dep_group(
        arena,
        names,
        values,
        vm.dependencies.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let optional_dependencies = build_dep_group(
        arena,
        names,
        values,
        vm.optional_dependencies
            .iter()
            .map(|(k, v)| (k.clone(), v.clone())),
    );

    // Peer dependencies: bun's ABI places **optional** peers at the FRONT of
    // the `peer_dependencies` array and stores the count of optional peers in
    // `non_optional_peer_dependencies_start` (i.e. the index where non-optional
    // peers begin).  Indices [0, start) are optional; [start, len) are required.
    // Source: npm.rs:688 comment + lockfile/Package.rs:841 reader condition.
    let optional_peer_set: std::collections::BTreeSet<&str> =
        vm.optional_peers.iter().map(|s| s.as_str()).collect();

    // Split peers: optional first, then non-optional (required).
    let opt_peers = vm
        .peer_dependencies
        .iter()
        .filter(|(k, _)| optional_peer_set.contains(k.as_str()));
    let non_opt_peers = vm
        .peer_dependencies
        .iter()
        .filter(|(k, _)| !optional_peer_set.contains(k.as_str()));

    // `non_optional_peer_dependencies_start` = number of optional peers
    // (= the index at which non-optional peers begin).
    let opt_count = vm
        .peer_dependencies
        .keys()
        .filter(|k| optional_peer_set.contains(k.as_str()))
        .count() as u32;

    // Build the combined peer group in one pass (optional then non-optional).
    let peer_dependencies = build_dep_group(
        arena,
        names,
        values,
        opt_peers
            .chain(non_opt_peers)
            .map(|(k, v)| (k.clone(), v.clone())),
    );

    // Integrity: decode SRI string → raw tag + digest bytes.
    let integrity = parse_integrity(&vm.integrity);

    // Tarball URL: intern the full URL string; bun reads it directly from the
    // PackageVersion when scheduling tarball downloads.
    let tarball_url = if vm.tarball_url.is_empty() {
        ExternalString::default()
    } else {
        arena.intern(&vm.tarball_url)
    };

    // OS and CPU bit-flag fields.
    let os = parse_os(&vm.os);
    let cpu = parse_cpu(&vm.cpu);

    // Bin entries.
    let bin = build_bin(vm, arena, bin_entries);

    PackageVersion {
        integrity,
        dependencies,
        optional_dependencies,
        peer_dependencies,
        non_optional_peer_dependencies_start: opt_count,
        tarball_url,
        os,
        cpu,
        has_install_script: vm.has_install_script,
        bin,
        ..PackageVersion::default()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Integrity (SRI → raw bytes)
// ──────────────────────────────────────────────────────────────────────────

fn parse_integrity(sri: &str) -> Integrity {
    if sri.is_empty() {
        return Integrity::default();
    }
    let (tag, b64) = if let Some(rest) = sri.strip_prefix("sha512-") {
        (IntegrityTag::SHA512, rest)
    } else if let Some(rest) = sri.strip_prefix("sha384-") {
        (IntegrityTag::SHA384, rest)
    } else if let Some(rest) = sri.strip_prefix("sha256-") {
        (IntegrityTag::SHA256, rest)
    } else if let Some(rest) = sri.strip_prefix("sha1-") {
        (IntegrityTag::SHA1, rest)
    } else {
        return Integrity::default();
    };

    let Some(decoded) = decode_base64(b64) else {
        return Integrity::default();
    };

    let mut value = [0u8; 64];
    let len = decoded.len().min(64);
    value[..len].copy_from_slice(&decoded[..len]);

    Integrity { tag, value }
}

/// Minimal standard-alphabet base64 decoder (no URL-safe; handles `=` padding).
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let input = input.trim_end_matches('=');
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i < bytes.len() {
        let remaining = bytes.len() - i;
        let a = b64_val(bytes[i])?;
        let b = if remaining > 1 {
            b64_val(bytes[i + 1])?
        } else {
            return None;
        };
        out.push((a << 2) | (b >> 4));
        if remaining > 2 {
            let c = b64_val(bytes[i + 2])?;
            out.push((b << 4) | (c >> 2));
            if remaining > 3 {
                let d = b64_val(bytes[i + 3])?;
                out.push((c << 6) | d);
            }
        }
        i += 4;
    }
    Some(out)
}

#[inline]
fn b64_val(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

// ──────────────────────────────────────────────────────────────────────────
// OS / CPU bit-flags
// ──────────────────────────────────────────────────────────────────────────

/// Fold an npm `"os"`/`"cpu"` token list into a bitset, porting bun's
/// `Negatable` accumulator + `combine`:
///
/// - `"any"` is a wildcard (→ all bits) and `"none"` an unrecognized value
///   (→ no bits), unless a later recognized token resets either flag;
/// - `"!name"` adds to a removed set;
/// - empty / only-unrecognized → NONE, only-removed → ALL minus removed,
///   only-added → added, mixed → added minus removed;
/// - an empty list means "all".
///
/// bun writes these token shapes back into `bun.lock` (`"none"`, one name,
/// one negated name, or an array), so round-tripping them exactly matters.
fn combine_negatable(list: &[String], all: u16, lookup: fn(&str) -> Option<u16>) -> u16 {
    let mut added: u16 = 0;
    let mut removed: u16 = 0;
    let mut had_wildcard = false;
    let mut had_unrecognized = false;

    for s in list {
        if s.is_empty() {
            continue;
        }
        if s == "any" {
            had_wildcard = true;
            continue;
        }
        if s == "none" {
            had_unrecognized = true;
            continue;
        }
        let (is_not, name) = match s.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, s.as_str()),
        };
        let Some(bit) = lookup(name) else {
            if !is_not {
                had_unrecognized = true;
            }
            continue;
        };
        // A recognized token resets the wildcard/unrecognized flags, so
        // ["any", "linux"] collapses to LINUX.
        had_wildcard = false;
        had_unrecognized = false;
        if is_not {
            removed |= bit;
        } else {
            added |= bit;
        }
    }

    let added = if had_wildcard { all } else { added };
    if added == 0 && removed == 0 {
        if had_unrecognized {
            return 0;
        }
        return all;
    }
    if added == 0 {
        return all & !removed;
    }
    if removed == 0 {
        return added;
    }
    added & !removed
}

fn os_bit(name: &str) -> Option<u16> {
    Some(match name {
        "aix" => OperatingSystem::AIX,
        "darwin" => OperatingSystem::DARWIN,
        "freebsd" => OperatingSystem::FREEBSD,
        "linux" => OperatingSystem::LINUX,
        "openbsd" => OperatingSystem::OPENBSD,
        "sunos" => OperatingSystem::SUNOS,
        "win32" => OperatingSystem::WIN32,
        "android" => OperatingSystem::ANDROID,
        _ => return None,
    })
}

fn cpu_bit(name: &str) -> Option<u16> {
    Some(match name {
        "arm" => Architecture::ARM,
        "arm64" => Architecture::ARM64,
        "ia32" => Architecture::IA32,
        "mips" => Architecture::MIPS,
        "mipsel" => Architecture::MIPSEL,
        "ppc" => Architecture::PPC,
        "ppc64" => Architecture::PPC64,
        "s390" => Architecture::S390,
        "s390x" => Architecture::S390X,
        "x32" => Architecture::X32,
        "x64" => Architecture::X64,
        _ => return None,
    })
}

/// Parse an npm `"os"` list into [`OperatingSystem`] bitflags.
fn parse_os(os: &[String]) -> OperatingSystem {
    OperatingSystem(combine_negatable(os, OperatingSystem::ALL_VALUE, os_bit))
}

/// Parse an npm `"cpu"` list into [`Architecture`] bitflags.
fn parse_cpu(cpu: &[String]) -> Architecture {
    Architecture(combine_negatable(cpu, Architecture::ALL_VALUE, cpu_bit))
}

// ──────────────────────────────────────────────────────────────────────────
// Bin
// ──────────────────────────────────────────────────────────────────────────

/// Build the [`Bin`] entry for one version.
///
/// Tag encoding (mirrors bun's `Bin.Tag`):
/// - `None (0)` — no bin field.
/// - `NamedFile (2)` — a single-entry map; value is two packed [`SemverString`]s
///   (key at `raw[0..1]`, path at `raw[2..3]`).
/// - `Map (4)` — multi-entry; `raw[0]` = offset into `bin_entries`,
///   `raw[1]` = count of `ExternalString` entries (2 × number of pairs).
fn build_bin(
    vm: &VersionMeta,
    arena: &mut StringArena,
    bin_entries: &mut Vec<ExternalString>,
) -> Bin {
    match vm.bin.len() {
        0 => Bin::default(),
        1 => {
            let (key, val) = vm.bin.iter().next().expect("len==1");
            let k_ss = arena.intern(key).value;
            let v_ss = arena.intern(val).value;
            Bin {
                tag: 2, // NamedFile
                _padding_tag: [0; 3],
                value: BinValue {
                    raw: [ss_lo(&k_ss), ss_hi(&k_ss), ss_lo(&v_ss), ss_hi(&v_ss)],
                },
            }
        }
        _ => {
            let off = bin_entries.len() as u32;
            let mut count: u32 = 0;
            for (key, val) in &vm.bin {
                bin_entries.push(arena.intern(key));
                bin_entries.push(arena.intern(val));
                count += 2;
            }
            Bin {
                tag: 4, // Map
                _padding_tag: [0; 3],
                value: BinValue {
                    raw: [off, count, 0, 0],
                },
            }
        }
    }
}

/// Low 32 bits of a [`SemverString`]'s bytes (as native-endian `u32`).
#[inline]
fn ss_lo(ss: &SemverString) -> u32 {
    u32::from_ne_bytes(ss.bytes[..4].try_into().unwrap())
}

/// High 32 bits of a [`SemverString`]'s bytes (as native-endian `u32`).
#[inline]
fn ss_hi(ss: &SemverString) -> u32 {
    u32::from_ne_bytes(ss.bytes[4..].try_into().unwrap())
}

// ──────────────────────────────────────────────────────────────────────────
// Semver helpers
// ──────────────────────────────────────────────────────────────────────────

/// A version string decomposed into its numeric triple and tag components.
/// `pre`/`build` are empty when the version has no such component.
pub(crate) struct ParsedVersion<'a> {
    pub(crate) major: u64,
    pub(crate) minor: u64,
    pub(crate) patch: u64,
    pub(crate) pre: &'a str,
    pub(crate) build: &'a str,
}

/// Parse `"major.minor.patch[-pre][+build]"`.  Unknown numeric components
/// default to 0; the tag portion follows bun's `Tag.parse` state machine so
/// the pre span (and therefore its hash) matches what bun computes when it
/// parses the same version at install time.
pub(crate) fn parse_version(s: &str) -> ParsedVersion<'_> {
    let mut parts = s.splitn(3, '.');
    let major: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let rest = parts.next().unwrap_or("");
    let digits_end = rest
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(rest.len());
    let patch: u64 = rest[..digits_end].parse().unwrap_or(0);
    let (pre, build) = parse_tag(&rest[digits_end..]);
    ParsedVersion {
        major,
        minor,
        patch,
        pre,
        build,
    }
}

/// Extract `(pre, build)` spans from the tag portion of a version string
/// (everything after the patch digits, including the leading `-`/`+`).
///
/// Port of bun's `Tag.parse` state machine: pre starts after the FIRST `-`
/// (later hyphens are kept inside the span, so `--canary.0` yields
/// `-canary.0`), build starts after the first `+`, and scanning stops at the
/// first character outside `[A-Za-z0-9.+-]`.
fn parse_tag(tag: &str) -> (&str, &str) {
    #[derive(PartialEq, Clone, Copy)]
    enum State {
        None,
        Pre,
        Build,
    }
    let mut pre = "";
    let mut build = "";
    let mut state = State::None;
    let mut start = 0usize;

    for (i, c) in tag.bytes().enumerate() {
        match c {
            b'+' => {
                if state == State::Pre {
                    pre = &tag[start..i];
                }
                if state != State::Build {
                    state = State::Build;
                    start = i + 1;
                }
            }
            b'-' => {
                if state != State::Pre {
                    state = State::Pre;
                    start = i + 1;
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' => {}
            _ => {
                match state {
                    State::None => {}
                    State::Pre => pre = &tag[start..i],
                    State::Build => build = &tag[start..i],
                }
                return (pre, build);
            }
        }
    }
    match state {
        State::None => {}
        State::Pre => pre = &tag[start..],
        State::Build => build = &tag[start..],
    }
    (pre, build)
}

/// Intern a tag component, or return the all-zero `ExternalString` (hash 0)
/// when the component is absent — the value bun's `Tag.eql` expects for a
/// tagless version.
fn intern_tag(arena: &mut StringArena, s: &str) -> ExternalString {
    if s.is_empty() {
        ExternalString::default()
    } else {
        arena.intern(s)
    }
}

#[inline]
fn triple(p: &ParsedVersion<'_>) -> (u64, u64, u64) {
    (p.major, p.minor, p.patch)
}

/// Order two pre-release tags by bun's `Tag.order_pre` rules: split on `.`,
/// compare segments numerically when both parse as integers, otherwise
/// bytewise (a numeric segment sorts before a non-numeric one); a longer tag
/// that shares its prefix sorts after the shorter one.
fn order_pre(lhs: &str, rhs: &str) -> Ordering {
    let mut lhs_itr = lhs.split('.');
    let mut rhs_itr = rhs.split('.');
    loop {
        match (lhs_itr.next(), rhs_itr.next()) {
            (None, None) => return Ordering::Equal,
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (Some(l), Some(r)) => {
                let l_uint: Option<u64> = l.parse().ok();
                let r_uint: Option<u64> = r.parse().ok();
                match (l_uint, r_uint) {
                    (Some(_), None) => return Ordering::Less,
                    (None, Some(_)) => return Ordering::Greater,
                    (Some(l), Some(r)) => match l.cmp(&r) {
                        Ordering::Equal => continue,
                        not_equal => return not_equal,
                    },
                    (None, None) => match l.cmp(r) {
                        Ordering::Equal => continue,
                        not_equal => return not_equal,
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_release() {
        let p = parse_version("1.2.3");
        assert_eq!((p.major, p.minor, p.patch), (1, 2, 3));
        assert_eq!(p.pre, "");
        assert_eq!(p.build, "");
    }

    #[test]
    fn parse_version_pre() {
        let p = parse_version("3.0.1-alpha.1");
        assert_eq!((p.major, p.minor, p.patch), (3, 0, 1));
        assert_eq!(p.pre, "alpha.1");
        assert_eq!(p.build, "");
    }

    #[test]
    fn parse_version_pre_and_build() {
        let p = parse_version("1.2.3-beta.2+exp.sha5114f8");
        assert_eq!((p.major, p.minor, p.patch), (1, 2, 3));
        assert_eq!(p.pre, "beta.2");
        assert_eq!(p.build, "exp.sha5114f8");
    }

    // bun keeps hyphens after the first: `--canary.67e7966.0` is the pre tag
    // `-canary.67e7966.0` (see the comment in bun's Tag.parse).
    #[test]
    fn parse_version_double_hyphen() {
        let p = parse_version("1.0.0--canary.67e7966.0");
        assert_eq!((p.major, p.minor, p.patch), (1, 0, 0));
        assert_eq!(p.pre, "-canary.67e7966.0");
    }

    #[test]
    fn parse_version_build_only() {
        let p = parse_version("1.0.0+20130313144700");
        assert_eq!(p.pre, "");
        assert_eq!(p.build, "20130313144700");
    }

    // The example from bun's order_pre comment:
    // 1.0.0-canary.0.0.0.0.0.0 < 1.0.0-canary.0.0.0.0.0.1
    #[test]
    fn order_pre_numeric_segments() {
        assert_eq!(
            order_pre("canary.0.0.0.0.0.0", "canary.0.0.0.0.0.1"),
            Ordering::Less
        );
    }

    #[test]
    fn order_pre_numeric_before_alpha() {
        // A segment that parses as an integer sorts before one that doesn't.
        assert_eq!(order_pre("1", "alpha"), Ordering::Less);
        assert_eq!(order_pre("alpha", "1"), Ordering::Greater);
    }

    #[test]
    fn order_pre_prefix_is_less() {
        assert_eq!(order_pre("alpha", "alpha.1"), Ordering::Less);
        assert_eq!(order_pre("alpha.1", "alpha"), Ordering::Greater);
    }

    #[test]
    fn order_pre_bytewise_alpha() {
        assert_eq!(order_pre("alpha", "beta"), Ordering::Less);
        assert_eq!(order_pre("beta.11", "beta.2"), Ordering::Greater);
    }

    fn strs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn negatable_empty_is_all() {
        assert_eq!(parse_os(&[]).0, OperatingSystem::ALL_VALUE);
        assert_eq!(parse_cpu(&[]).0, Architecture::ALL_VALUE);
    }

    // bun serializes a NONE bitset back to bun.lock as the string "none".
    #[test]
    fn negatable_none_token() {
        assert_eq!(parse_os(&strs(&["none"])).0, 0);
        assert_eq!(parse_cpu(&strs(&["none"])).0, 0);
    }

    #[test]
    fn negatable_single_and_negated() {
        assert_eq!(parse_os(&strs(&["linux"])).0, OperatingSystem::LINUX);
        assert_eq!(
            parse_os(&strs(&["!win32"])).0,
            OperatingSystem::ALL_VALUE & !OperatingSystem::WIN32
        );
        assert_eq!(parse_cpu(&strs(&["x64", "!arm64"])).0, Architecture::X64);
    }

    // ["any", "linux"] collapses to LINUX: a recognized token resets the
    // wildcard flag (bun's Negatable.apply).
    #[test]
    fn negatable_wildcard_reset() {
        assert_eq!(parse_os(&strs(&["any", "linux"])).0, OperatingSystem::LINUX);
        assert_eq!(parse_os(&strs(&["any"])).0, OperatingSystem::ALL_VALUE);
    }

    #[test]
    fn negatable_unrecognized_is_none() {
        assert_eq!(parse_os(&strs(&["macos"])).0, 0);
    }
}
