//! bun npm `.npm` manifest-cache (`bun-npm-manifest-cache-v0.0.7`) support:
//! verbatim layout structs ([`layout`]), the serializer ([`serialize`]), a
//! minimal single-version builder, a round-trip reader, serde metadata types
//! ([`meta`]), and the general multi-version builder ([`build`]).

pub mod build;
pub mod layout;
pub mod meta;
pub mod serialize;

use std::collections::BTreeMap;

use crate::wyhash::wyhash11;
use layout::{
    DistTagMap, ExternVersionMap, ExternalString, ExternalStringList, ExternalStringMap, Integrity,
    IntegrityTag, NpmPackage, PackageVersion, PackageVersionList, SemverString, SemverVersion,
    VersionSlice,
};
use serialize::PackageManifest;

/// bun's default registry URL (`Registry::DEFAULT_URL`), with trailing slash.
pub const DEFAULT_REGISTRY_URL: &str = "https://registry.npmjs.org/";

/// `wyhash11(0, "https://registry.npmjs.org")` — bun's `DEFAULT_URL_HASH`.
///
/// Hardcoded from the literal observed in real `.npm` headers (bytes 49..57)
/// and verified at test time against [`default_url_hash`].
pub const DEFAULT_URL_HASH: u64 = 0x9c1e_4d1f_1eff_5fcd;

/// Length of the default registry URL with the trailing slash removed
/// (`len("https://registry.npmjs.org") == 26`). This is the value bun stores in
/// the `.npm` header right after `url_hash`.
pub const DEFAULT_REGISTRY_HREF_LEN: u64 = 26;

/// Strip trailing `/` and `\` from a registry href while more than one byte
/// remains — the bytes bun hashes and measures for the `.npm` header.
pub fn without_trailing_slash(href: &str) -> &str {
    let bytes = href.as_bytes();
    let mut end = href.len();
    while end > 1 && matches!(bytes[end - 1], b'/' | b'\\') {
        end -= 1;
    }
    &href[..end]
}

/// wyhash11 of a registry href without its trailing slash — the `url_hash`
/// stored in the `.npm` header and in non-default manifest filenames.
pub fn url_hash(href: &str) -> u64 {
    wyhash11(0, without_trailing_slash(href).as_bytes())
}

/// Registry href length (without trailing slash) stored in the `.npm` header
/// right after `url_hash`.
pub fn registry_href_len(href: &str) -> u64 {
    without_trailing_slash(href).len() as u64
}

/// wyhash11 of the default registry URL **without** the trailing slash — matches
/// bun's `DEFAULT_URL_HASH`.
pub fn default_url_hash() -> u64 {
    url_hash(DEFAULT_REGISTRY_URL)
}

/// `.npm` filename for a package: `<hex16(wyhash11(name))>.npm` on the
/// default registry, `<hex16(wyhash11(name))>-<hex16(url_hash)>.npm` for a
/// non-default registry.
pub fn manifest_file_name(name: &str, registry: Option<&str>) -> String {
    let file_id = wyhash11(0, name.as_bytes());
    match registry {
        None => format!("{file_id:016x}.npm"),
        Some(href) => format!("{file_id:016x}-{:016x}.npm", url_hash(href)),
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Minimal single-version builder (spike quality; Task 4 generalizes it).
// ──────────────────────────────────────────────────────────────────────────

/// A single (name, version-range) dependency pair, e.g.
/// `("svelte", "^3.0.0 || ^4.0.0 || ^5.0.0")`.
#[derive(Clone)]
pub struct Dep {
    pub name: String,
    pub range: String,
}

/// Inputs for [`build_single_version`].
pub struct SingleVersionInput<'a> {
    pub name: &'a str,
    pub version: (u64, u64, u64),
    /// Optional SHA-512 digest (raw 64 bytes) for the version's integrity field.
    pub sha512: Option<[u8; 64]>,
    pub dependencies: Vec<Dep>,
    pub optional_dependencies: Vec<Dep>,
    pub peer_dependencies: Vec<Dep>,
}

/// Helper that accumulates the string buffer and the two external-string arrays
/// while building a manifest.
#[derive(Default)]
pub(crate) struct StringArena {
    pub(crate) buf: Vec<u8>,
}

impl StringArena {
    /// Append `s` to the buffer if it cannot be stored inline, returning the
    /// `ExternalString` handle (inline or external) for it.
    pub(crate) fn intern(&mut self, s: &str) -> ExternalString {
        let bytes = s.as_bytes();
        let hash = wyhash11(0, bytes);
        let value = if SemverString::can_inline(bytes) {
            SemverString::init_inline(bytes)
        } else {
            let off = self.buf.len() as u32;
            self.buf.extend_from_slice(bytes);
            SemverString::init_external(off, bytes.len() as u32)
        };
        ExternalString { value, hash }
    }
}

/// Append a dependency group (name → version-range pairs) to the shared
/// `names` and `values` vectors, returning the `ExternalStringMap` slice that
/// indexes them.  Both vectors grow monotonically across calls, so offsets from
/// earlier calls remain stable.
pub(crate) fn build_dep_group(
    arena: &mut StringArena,
    names: &mut Vec<ExternalString>,
    values: &mut Vec<ExternalString>,
    deps: impl Iterator<Item = (String, String)>,
) -> ExternalStringMap {
    let name_off = names.len() as u32;
    let value_off = values.len() as u32;
    let mut count: u32 = 0;
    for (name, range) in deps {
        names.push(arena.intern(&name));
        values.push(arena.intern(&range));
        count += 1;
    }
    if count == 0 {
        return ExternalStringMap::default();
    }
    ExternalStringMap {
        name: ExternalStringList::new(name_off, count),
        value: ExternalStringList::new(value_off, count),
    }
}

/// Build an in-memory [`PackageManifest`] for a single version of an
/// npm-registry package, populating just the fields bun reads for
/// (peer-)dependency resolution: name, the `releases` map, and each dependency
/// group of the one `PackageVersion`.
///
/// For multi-version packages, prefer [`build::build_manifest`] with a
/// [`meta::PackageMeta`] instead.
pub fn build_single_version(input: &SingleVersionInput) -> PackageManifest {
    let mut arena = StringArena::default();

    // Package name.
    let name_ext = arena.intern(input.name);

    // Dependency groups: names go into `external_strings`, values (ranges) go
    // into `external_strings_for_versions` — matching bun's two-buffer split.
    let mut names: Vec<ExternalString> = Vec::new();
    let mut values: Vec<ExternalString> = Vec::new();

    let dependencies = build_dep_group(
        &mut arena,
        &mut names,
        &mut values,
        input.dependencies.iter().map(|d| (d.name.clone(), d.range.clone())),
    );
    let optional_dependencies = build_dep_group(
        &mut arena,
        &mut names,
        &mut values,
        input.optional_dependencies.iter().map(|d| (d.name.clone(), d.range.clone())),
    );
    let peer_dependencies = build_dep_group(
        &mut arena,
        &mut names,
        &mut values,
        input.peer_dependencies.iter().map(|d| (d.name.clone(), d.range.clone())),
    );

    let integrity = match input.sha512 {
        Some(value) => Integrity {
            tag: IntegrityTag::SHA512,
            value,
        },
        None => Integrity::default(),
    };

    let pv = PackageVersion {
        integrity,
        dependencies,
        optional_dependencies,
        peer_dependencies,
        ..PackageVersion::default()
    };

    let version = SemverVersion {
        major: input.version.0,
        minor: input.version.1,
        patch: input.version.2,
        ..SemverVersion::default()
    };

    let versions = vec![version].into_boxed_slice();
    let package_versions = vec![pv].into_boxed_slice();
    let n_names = names.len() as u32;
    let external_strings = names.into_boxed_slice();
    let external_strings_for_versions = values.into_boxed_slice();

    let mut pkg = NpmPackage {
        name: name_ext,
        // Far-future expiry so bun treats the manifest as "fresh" under
        // `BUN_MANIFEST_CACHE=2` (cache-control on) and never revalidates over
        // the network: `by_name_hash` only returns a non-expired manifest when
        // `public_max_age > timestamp_for_manifest_cache_control` (current time).
        public_max_age: u32::MAX,
        ..NpmPackage::default()
    };
    // releases: keys index the `versions` buffer, values index `package_versions`.
    pkg.releases = ExternVersionMap {
        keys: VersionSlice::new(0, 1),
        values: PackageVersionList::new(0, 1),
    };
    pkg.prereleases = ExternVersionMap::default();
    pkg.dist_tags = DistTagMap::default();
    pkg.versions_buf = VersionSlice::new(0, 1);
    pkg.string_lists_buf = ExternalStringList::new(0, n_names);

    PackageManifest {
        pkg,
        string_buf: arena.buf.into_boxed_slice(),
        versions,
        external_strings,
        external_strings_for_versions,
        package_versions,
        extern_strings_bin_entries: Box::new([]),
        bundled_deps_buf: Box::new([]),
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Round-trip reader (ports `Serializer::read_array` + `read_all`).
// ──────────────────────────────────────────────────────────────────────────

/// A deserialized manifest with the same buffers as [`PackageManifest`].
#[derive(Default)]
pub struct ReadManifest {
    pub url_hash: u64,
    pub href_len: u64,
    pub pkg: NpmPackage,
    pub string_buf: Vec<u8>,
    pub versions: Vec<SemverVersion>,
    pub external_strings: Vec<ExternalString>,
    pub external_strings_for_versions: Vec<ExternalString>,
    pub package_versions: Vec<PackageVersion>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn read_u64(&mut self) -> u64 {
        let v = u64::from_le_bytes(self.bytes[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        v
    }

    fn align_to(&mut self, align: usize) {
        self.pos = self.pos.next_multiple_of(align);
    }

    /// Mirror of `read_struct::<NpmPackage>` after aligning.
    fn read_struct<T: Copy>(&mut self) -> T {
        self.align_to(std::mem::align_of::<T>());
        // SAFETY: bytes were produced by our own serializer for a POD `T`.
        let v = unsafe { std::ptr::read_unaligned(self.bytes[self.pos..].as_ptr().cast::<T>()) };
        self.pos += std::mem::size_of::<T>();
        v
    }

    /// Mirror of `Serializer::read_array`.
    fn read_array<T: Copy>(&mut self) -> Vec<T> {
        let byte_len = self.read_u64() as usize;
        if byte_len == 0 {
            return Vec::new();
        }
        self.align_to(std::mem::align_of::<T>());
        let region = &self.bytes[self.pos..self.pos + byte_len];
        let n = byte_len / std::mem::size_of::<T>();
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            // SAFETY: region is `byte_len` long, n elements of size_of::<T>.
            let v = unsafe {
                std::ptr::read_unaligned(region.as_ptr().add(i * std::mem::size_of::<T>()).cast::<T>())
            };
            out.push(v);
        }
        self.pos += byte_len;
        out
    }
}

/// Deserialize a `.npm` byte buffer back into a [`ReadManifest`]. Returns `None`
/// if the header does not match.
pub fn read(bytes: &[u8]) -> Option<ReadManifest> {
    if bytes.len() < serialize::HEADER.len() || &bytes[..serialize::HEADER.len()] != serialize::HEADER
    {
        return None;
    }
    let mut r = Reader {
        bytes,
        pos: serialize::HEADER.len(),
    };
    let url_hash = r.read_u64();
    let href_len = r.read_u64();
    let pkg: NpmPackage = r.read_struct();
    let string_buf = r.read_array::<u8>();
    let versions = r.read_array::<SemverVersion>();
    let external_strings = r.read_array::<ExternalString>();
    let external_strings_for_versions = r.read_array::<ExternalString>();
    let package_versions = r.read_array::<PackageVersion>();
    let _bin = r.read_array::<ExternalString>();
    let _bundled = r.read_array::<u64>();

    Some(ReadManifest {
        url_hash,
        href_len,
        pkg,
        string_buf,
        versions,
        external_strings,
        external_strings_for_versions,
        package_versions,
    })
}

/// Resolve an [`ExternalString`]'s text against `string_buf` (inline or external).
pub fn resolve_str<'a>(s: &'a ExternalString, string_buf: &'a [u8]) -> Vec<u8> {
    let bytes = &s.value.bytes;
    if bytes[SemverString::MAX_INLINE_LEN - 1] & 0x80 == 0 {
        // inline: bytes up to first NUL (or all 8)
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(8);
        bytes[..end].to_vec()
    } else {
        let bits = u64::from_ne_bytes(*bytes) & ((1u64 << 63) - 1);
        let off = (bits & 0xffff_ffff) as usize;
        let len = (bits >> 32) as usize;
        string_buf[off..off + len].to_vec()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// ReadManifest higher-level accessors (used by tests and downstream crates).
// ──────────────────────────────────────────────────────────────────────────

/// A resolved view of one `PackageVersion` within a deserialized manifest.
pub struct ReadPackageVersion<'a> {
    manifest: &'a ReadManifest,
    pub pv: PackageVersion,
}

impl<'a> ReadPackageVersion<'a> {
    /// All peer dependencies as a `BTreeMap<name, range>`.
    pub fn peer_dependencies(&self) -> BTreeMap<String, String> {
        self.dep_map(self.pv.peer_dependencies)
    }

    /// All regular (non-optional) dependencies as a `BTreeMap<name, range>`.
    pub fn dependencies(&self) -> BTreeMap<String, String> {
        self.dep_map(self.pv.dependencies)
    }

    /// All optional dependencies as a `BTreeMap<name, range>`.
    pub fn optional_dependencies(&self) -> BTreeMap<String, String> {
        self.dep_map(self.pv.optional_dependencies)
    }

    /// Full tarball URL stored in this `PackageVersion`, or an empty string if
    /// the URL was not stored (bun infers it from the registry in that case).
    pub fn tarball_url(&self) -> String {
        let bytes = resolve_str(&self.pv.tarball_url, &self.manifest.string_buf);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Resolve the name of the peer dependency at position `i` within the
    /// `peer_dependencies` array.  Used by tests that validate the bun ABI
    /// ordering (optional peers first, non-optional peers after
    /// `non_optional_peer_dependencies_start`).
    pub fn peer_dep_name_at(&self, i: usize) -> String {
        let n_off = self.pv.peer_dependencies.name.off as usize;
        let bytes = resolve_str(
            &self.manifest.external_strings[n_off + i],
            &self.manifest.string_buf,
        );
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn dep_map(&self, map: ExternalStringMap) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        let n_off = map.name.off as usize;
        let v_off = map.value.off as usize;
        for i in 0..map.name.len as usize {
            let name_bytes =
                resolve_str(&self.manifest.external_strings[n_off + i], &self.manifest.string_buf);
            let val_bytes = resolve_str(
                &self.manifest.external_strings_for_versions[v_off + i],
                &self.manifest.string_buf,
            );
            result.insert(
                String::from_utf8_lossy(&name_bytes).into_owned(),
                String::from_utf8_lossy(&val_bytes).into_owned(),
            );
        }
        result
    }
}

impl ReadManifest {
    /// Package name as raw UTF-8 bytes.
    pub fn name(&self) -> Vec<u8> {
        resolve_str(&self.pkg.name, &self.string_buf)
    }

    /// Cache-control max-age (seconds).  `u32::MAX` means "never expires".
    pub fn public_max_age(&self) -> u32 {
        self.pkg.public_max_age
    }

    /// Look up a version by its semver string (e.g. `"2.2.2"` or
    /// `"3.0.1-alpha.1"`), mirroring bun's `find_by_version`: the map is
    /// chosen by whether the version carries a pre-release tag, and equality
    /// compares the numeric triple plus the wyhash of the pre tag.
    pub fn find_version(&self, version_str: &str) -> Option<ReadPackageVersion<'_>> {
        let v = build::parse_version(version_str);
        let map = if v.pre.is_empty() {
            self.pkg.releases
        } else {
            self.pkg.prereleases
        };
        // Hash 0 is what bun's parser leaves in `tag.pre` for tagless versions.
        let pre_hash = if v.pre.is_empty() {
            0
        } else {
            wyhash11(0, v.pre.as_bytes())
        };

        let keys_slice =
            &self.versions[map.keys.off as usize..(map.keys.off + map.keys.len) as usize];
        let pvs_slice = &self.package_versions
            [map.values.off as usize..(map.values.off + map.values.len) as usize];

        for (i, k) in keys_slice.iter().enumerate() {
            if k.major == v.major
                && k.minor == v.minor
                && k.patch == v.patch
                && k.tag.pre.hash == pre_hash
            {
                return Some(ReadPackageVersion {
                    manifest: self,
                    pv: pvs_slice[i],
                });
            }
        }
        None
    }
}
