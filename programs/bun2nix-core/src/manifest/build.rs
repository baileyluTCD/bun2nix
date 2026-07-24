//! General multi-version `.npm` manifest builder.
//!
//! [`build_manifest`] converts a [`meta::PackageMeta`] (which may contain
//! multiple versions) into a [`serialize::PackageManifest`] suitable for
//! writing with [`serialize::write`].
//!
//! The implementation reuses [`super::StringArena`] and
//! [`super::build_dep_group`] — the same interning primitives used by
//! [`super::build_single_version`] — so string-buffer logic lives in one place.

use super::{
    StringArena, build_dep_group,
    layout::{
        Architecture, Bin, BinValue, DistTagMap, ExternVersionMap, ExternalString,
        ExternalStringList, Integrity, IntegrityTag, NpmPackage, OperatingSystem, PackageVersion,
        PackageVersionList, SemverString, SemverVersion, VersionSlice,
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

    // Sort versions ascending (deterministic output regardless of JSON order).
    let mut sorted: Vec<&VersionMeta> = pkg.versions.iter().collect();
    sorted.sort_by_key(|v| parse_semver(&v.version));

    let mut semver_versions: Vec<SemverVersion> = Vec::with_capacity(sorted.len());
    let mut package_versions: Vec<PackageVersion> = Vec::with_capacity(sorted.len());

    for vm in &sorted {
        let (major, minor, patch) = parse_semver(&vm.version);
        semver_versions.push(SemverVersion {
            major,
            minor,
            patch,
            ..SemverVersion::default()
        });

        let pv = build_one_version(vm, &mut arena, &mut names, &mut values, &mut bin_entries);
        package_versions.push(pv);
    }

    let n = sorted.len() as u32;
    let n_names = names.len() as u32;
    let external_strings = names.into_boxed_slice();
    let external_strings_for_versions = values.into_boxed_slice();
    let extern_strings_bin_entries = bin_entries.into_boxed_slice();

    let mut pkg_struct = NpmPackage {
        name: name_ext,
        // Never expire — so bun treats this manifest as fresh indefinitely
        // under BUN_MANIFEST_CACHE=2 (verified in the Task 3 spike).
        public_max_age: u32::MAX,
        ..NpmPackage::default()
    };

    // releases: keys = entire versions array, values = entire package_versions array.
    pkg_struct.releases = ExternVersionMap {
        keys: VersionSlice::new(0, n),
        values: PackageVersionList::new(0, n),
    };
    pkg_struct.prereleases = ExternVersionMap::default();
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
        vm.optional_dependencies.iter().map(|(k, v)| (k.clone(), v.clone())),
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
    let opt_count =
        vm.peer_dependencies.keys().filter(|k| optional_peer_set.contains(k.as_str())).count()
            as u32;

    // Build the combined peer group in one pass (optional then non-optional).
    let peer_dependencies = build_dep_group(
        arena,
        names,
        values,
        opt_peers.chain(non_opt_peers).map(|(k, v)| (k.clone(), v.clone())),
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
        let b = if remaining > 1 { b64_val(bytes[i + 1])? } else { return None };
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

/// Parse an npm `"os"` array into [`OperatingSystem`] bitflags.
/// An empty list means "all OSes" (`OperatingSystem::ALL`).
fn parse_os(os: &[String]) -> OperatingSystem {
    if os.is_empty() {
        return OperatingSystem::ALL;
    }
    let mut flags: u16 = 0;
    for s in os {
        flags |= match s.as_str() {
            "aix" => OperatingSystem::AIX,
            "darwin" | "macos" => OperatingSystem::DARWIN,
            "freebsd" => OperatingSystem::FREEBSD,
            "linux" => OperatingSystem::LINUX,
            "openbsd" => OperatingSystem::OPENBSD,
            "sunos" => OperatingSystem::SUNOS,
            "win32" => OperatingSystem::WIN32,
            "android" => OperatingSystem::ANDROID,
            _ => 0,
        };
    }
    OperatingSystem(flags)
}

/// Parse an npm `"cpu"` array into [`Architecture`] bitflags.
/// An empty list means "all architectures" (`Architecture::ALL`).
fn parse_cpu(cpu: &[String]) -> Architecture {
    if cpu.is_empty() {
        return Architecture::ALL;
    }
    let mut flags: u16 = 0;
    for s in cpu {
        flags |= match s.as_str() {
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
            _ => 0,
        };
    }
    Architecture(flags)
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
                    raw: [
                        ss_lo(&k_ss),
                        ss_hi(&k_ss),
                        ss_lo(&v_ss),
                        ss_hi(&v_ss),
                    ],
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
                value: BinValue { raw: [off, count, 0, 0] },
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

/// Parse `"major.minor.patch"` (optionally with a pre-release suffix) into a
/// sortable `(major, minor, patch)` triple.  Unknown components default to 0.
pub(crate) fn parse_semver(s: &str) -> (u64, u64, u64) {
    let mut parts = s.splitn(3, '.');
    let major: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch: u64 = parts
        .next()
        .and_then(|p| p.split('-').next()?.parse().ok())
        .unwrap_or(0);
    (major, minor, patch)
}
