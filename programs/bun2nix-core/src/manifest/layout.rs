//! POD structs ported **verbatim** from bun's `bun-npm-manifest-cache-v0.0.7`
//! on-disk format. Layout is an ABI contract: these structs are reinterpreted
//! as raw bytes by the serializer, so size/alignment/field-offsets must match
//! bun exactly. Every `const _: () = { … }` block below is a transcription
//! guard — if one fails to compile, a layout was copied wrong.
//!
//! Sources (bun @ 5621c5de90):
//! - `DistTagMap`, `ExternVersionMap`, `PackageVersion`, `NpmPackage` — `src/install/npm.rs`
//! - `ExternalSlice`, `ExternalStringMap`, OS/CPU/Libc — `src/install_types/resolver_hooks.rs`
//! - `ExternalString`, `SemverString` (`String`), `Pointer` — `src/semver/lib.rs`
//! - `VersionType<u64>` (`Semver::Version`), `Tag` — `src/semver/Version.rs`
//! - `Integrity` (+ `Tag`) — `src/install/integrity.rs`
//! - `Bin` (+ `Value` union, `Tag`) — `src/install/bin.rs`

use core::marker::PhantomData;
use core::mem::{offset_of, size_of};

// ──────────────────────────────────────────────────────────────────────────
// SemverString (`Semver::semver_string::String`) — src/semver/lib.rs:238
//
// 8 raw bytes. Either an inline string (final bit of byte 7 clear, NUL
// terminated when < 8 bytes) or an external `Pointer` (final bit set, low 63
// bits are the `{off,len}` pair into the string buffer).
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Default)]
pub struct SemverString {
    pub bytes: [u8; SemverString::MAX_INLINE_LEN],
}

impl SemverString {
    pub const MAX_INLINE_LEN: usize = 8;

    pub const EMPTY: SemverString = SemverString { bytes: [0; 8] };

    /// Pointers are truncated to 63 bits via this mask (bun: `MAX_ADDRESSABLE_SPACE_MASK`).
    const MAX_ADDRESSABLE_SPACE_MASK: u64 = (1u64 << 63) - 1;

    #[inline]
    pub fn can_inline(buf: &[u8]) -> bool {
        match buf.len() {
            0..=7 => true,
            8 => buf[7] & 0x80 == 0,
            _ => false,
        }
    }

    /// Build an inline string (`String::init_inline`). Caller guarantees
    /// `can_inline(in_)`.
    #[inline]
    pub fn init_inline(in_: &[u8]) -> SemverString {
        debug_assert!(Self::can_inline(in_));
        let mut bytes = [0u8; 8];
        bytes[..in_.len()].copy_from_slice(in_);
        SemverString { bytes }
    }

    /// Build an external string referencing `string_buf[off..off+len]`. Mirrors
    /// the pointer-packing path of `String::init`: `off` in the low 4 bytes,
    /// `len` in the high 4 bytes, with bit 63 set to mark "external".
    #[inline]
    pub fn init_external(off: u32, len: u32) -> SemverString {
        let bits = (off as u64) | ((len as u64) << 32);
        let packed = (bits & Self::MAX_ADDRESSABLE_SPACE_MASK) | (1u64 << 63);
        SemverString {
            bytes: packed.to_ne_bytes(),
        }
    }

    /// Construct the right representation for `in_` placed at `off` in the
    /// string buffer.
    #[inline]
    pub fn init(off: u32, in_: &[u8]) -> SemverString {
        if Self::can_inline(in_) {
            Self::init_inline(in_)
        } else {
            Self::init_external(off, in_.len() as u32)
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// String.Pointer — src/semver/lib.rs:803
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct Pointer {
    pub off: u32,
    pub len: u32,
}

// ──────────────────────────────────────────────────────────────────────────
// ExternalString — src/semver/lib.rs:165
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ExternalString {
    pub value: SemverString,
    pub hash: u64,
}

const _: () = assert!(size_of::<ExternalString>() == 16);

// ──────────────────────────────────────────────────────────────────────────
// Semver::Version == VersionType<u64> — src/semver/Version.rs:54
//
// v0.0.7 uses u64 for major/minor/patch, so `_tag_padding` is `[u8; 0]` and is
// omitted here. `tag` sits at offset 24, total size 56.
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct Tag {
    pub pre: ExternalString,
    pub build: ExternalString,
}

const _: () = {
    assert!(size_of::<Tag>() == 32);
    assert!(align_of::<Tag>() == 8);
};

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SemverVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub tag: Tag,
}

// Layout is load-bearing (lockfile/manifest binary format).
const _: () = {
    assert!(size_of::<SemverVersion>() == 56);
    assert!(offset_of!(SemverVersion, tag) == 24);
};

// ──────────────────────────────────────────────────────────────────────────
// ExternalSlice<T> + aliases — src/install_types/resolver_hooks.rs:41
//
// An `(off, len)` index pair into a flat backing buffer. Manual trait impls
// (matching bun) so a non-`Copy` element type would not gate the `(off,len)`
// pair's own copyability.
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
pub struct ExternalSlice<T> {
    pub off: u32,
    pub len: u32,
    _marker: PhantomData<T>,
}

impl<T> Copy for ExternalSlice<T> {}
impl<T> Clone for ExternalSlice<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Default for ExternalSlice<T> {
    #[inline]
    fn default() -> Self {
        Self {
            off: 0,
            len: 0,
            _marker: PhantomData,
        }
    }
}

impl<T> ExternalSlice<T> {
    #[inline]
    pub const fn new(off: u32, len: u32) -> Self {
        Self {
            off,
            len,
            _marker: PhantomData,
        }
    }

    pub const INVALID: Self = Self {
        off: u32::MAX,
        len: u32::MAX,
        _marker: PhantomData,
    };

    #[inline]
    pub fn is_invalid(self) -> bool {
        self.off == u32::MAX && self.len == u32::MAX
    }
}

pub type PackageNameHash = u64;
pub type ExternalStringList = ExternalSlice<ExternalString>;
pub type ExternalPackageNameHashList = ExternalSlice<PackageNameHash>;
pub type VersionSlice = ExternalSlice<SemverVersion>;
pub type PackageVersionList = ExternalSlice<PackageVersion>;

const _: () = assert!(size_of::<ExternalStringList>() == 8);

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct ExternalStringMap {
    pub name: ExternalStringList,
    pub value: ExternalStringList,
}

const _: () = assert!(size_of::<ExternalStringMap>() == 16);

// ──────────────────────────────────────────────────────────────────────────
// OperatingSystem / Architecture / Libc — src/install_types/resolver_hooks.rs
// transparent newtypes over u16/u16/u8.
// ──────────────────────────────────────────────────────────────────────────

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct OperatingSystem(pub u16);

impl OperatingSystem {
    pub const NONE: Self = Self(0);
    pub const AIX: u16 = 1 << 1;
    pub const DARWIN: u16 = 1 << 2;
    pub const FREEBSD: u16 = 1 << 3;
    pub const LINUX: u16 = 1 << 4;
    pub const OPENBSD: u16 = 1 << 5;
    pub const SUNOS: u16 = 1 << 6;
    pub const WIN32: u16 = 1 << 7;
    pub const ANDROID: u16 = 1 << 8;
    pub const ALL_VALUE: u16 = Self::AIX
        | Self::DARWIN
        | Self::FREEBSD
        | Self::LINUX
        | Self::OPENBSD
        | Self::SUNOS
        | Self::WIN32
        | Self::ANDROID;
    pub const ALL: Self = Self(Self::ALL_VALUE);
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Architecture(pub u16);

impl Architecture {
    pub const NONE: Self = Self(0);
    pub const ARM: u16 = 1 << 1;
    pub const ARM64: u16 = 1 << 2;
    pub const IA32: u16 = 1 << 3;
    pub const MIPS: u16 = 1 << 4;
    pub const MIPSEL: u16 = 1 << 5;
    pub const PPC: u16 = 1 << 6;
    pub const PPC64: u16 = 1 << 7;
    pub const S390: u16 = 1 << 8;
    pub const S390X: u16 = 1 << 9;
    pub const X32: u16 = 1 << 10;
    pub const X64: u16 = 1 << 11;
    pub const ALL_VALUE: u16 = Self::ARM
        | Self::ARM64
        | Self::IA32
        | Self::MIPS
        | Self::MIPSEL
        | Self::PPC
        | Self::PPC64
        | Self::S390
        | Self::S390X
        | Self::X32
        | Self::X64;
    pub const ALL: Self = Self(Self::ALL_VALUE);
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Libc(pub u8);

impl Libc {
    pub const NONE: Self = Self(0);
    pub const GLIBC: u8 = 1 << 1;
    pub const MUSL: u8 = 1 << 2;
    pub const ALL_VALUE: u8 = Self::GLIBC | Self::MUSL;
    pub const ALL: Self = Self(Self::ALL_VALUE);
}

// ──────────────────────────────────────────────────────────────────────────
// Integrity — src/install/integrity.rs:15  (size 65, align 1)
// ──────────────────────────────────────────────────────────────────────────

/// `#[repr(transparent)]` newtype over `u8` (any byte is a valid tag).
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct IntegrityTag(pub u8);

impl IntegrityTag {
    pub const UNKNOWN: IntegrityTag = IntegrityTag(0);
    pub const SHA1: IntegrityTag = IntegrityTag(1);
    pub const SHA256: IntegrityTag = IntegrityTag(2);
    pub const SHA384: IntegrityTag = IntegrityTag(3);
    pub const SHA512: IntegrityTag = IntegrityTag(4);
}

impl Default for IntegrityTag {
    fn default() -> Self {
        IntegrityTag::UNKNOWN
    }
}

/// `max(SHA1=20, SHA256=32, SHA384=48, SHA512=64) = 64`.
pub const DIGEST_BUF_LEN: usize = 64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Integrity {
    pub tag: IntegrityTag,
    pub value: [u8; DIGEST_BUF_LEN],
}

impl Default for Integrity {
    fn default() -> Self {
        Self {
            tag: IntegrityTag::UNKNOWN,
            value: [0u8; DIGEST_BUF_LEN],
        }
    }
}

const _: () = assert!(size_of::<Integrity>() == 65);
const _: () = assert!(align_of::<Integrity>() == 1);

// ──────────────────────────────────────────────────────────────────────────
// Bin — src/install/bin.rs:40  (tag + 3 padding + 16-byte union value)
//
// The real `Value` is a `#[repr(C)] union` whose largest member is
// `[SemverString; 2]` (16 bytes) and whose alignment (4) comes from
// `ExternalStringList`. Modelled here as a 16-byte, align-4 POD so `Bin` has
// size 20 / align 4, matching bun. We only ever emit `Tag::None` (zeroed).
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BinValue {
    pub raw: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bin {
    pub tag: u8,
    pub _padding_tag: [u8; 3],
    pub value: BinValue,
}

impl Bin {
    pub const TAG_NONE: u8 = 0;
}

impl Default for Bin {
    fn default() -> Self {
        Bin {
            tag: Bin::TAG_NONE,
            _padding_tag: [0; 3],
            value: BinValue::default(),
        }
    }
}

const _: () = {
    assert!(size_of::<Bin>() == 20);
    assert!(align_of::<Bin>() == 4);
};

// ──────────────────────────────────────────────────────────────────────────
// DistTagMap — src/install/npm.rs:586
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct DistTagMap {
    pub tags: ExternalStringList,
    pub versions: VersionSlice,
}

// ──────────────────────────────────────────────────────────────────────────
// ExternVersionMap — src/install/npm.rs:595
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct ExternVersionMap {
    pub keys: VersionSlice,
    pub values: PackageVersionList,
}

// ──────────────────────────────────────────────────────────────────────────
// PackageVersion (240 B) — src/install/npm.rs:650
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PackageVersion {
    pub integrity: Integrity,
    pub _padding_after_integrity: [u8; 3],
    pub dependencies: ExternalStringMap,
    pub optional_dependencies: ExternalStringMap,
    pub peer_dependencies: ExternalStringMap,
    pub dev_dependencies: ExternalStringMap,
    pub bundled_dependencies: ExternalPackageNameHashList,
    pub bin: Bin,
    pub engines: ExternalStringMap,
    pub non_optional_peer_dependencies_start: u32,
    pub _padding_before_man_dir: [u8; 4],
    pub man_dir: ExternalString,
    pub tarball_url: ExternalString,
    pub unpacked_size: u32,
    pub file_count: u32,
    pub os: OperatingSystem,
    pub cpu: Architecture,
    pub libc: Libc,
    pub has_install_script: bool,
    pub _padding_tail: [u8; 2],
    pub publish_timestamp_ms: f64,
}

impl Default for PackageVersion {
    fn default() -> Self {
        Self {
            integrity: Integrity::default(),
            _padding_after_integrity: [0; 3],
            dependencies: ExternalStringMap::default(),
            optional_dependencies: ExternalStringMap::default(),
            peer_dependencies: ExternalStringMap::default(),
            dev_dependencies: ExternalStringMap::default(),
            bundled_dependencies: ExternalPackageNameHashList::default(),
            bin: Bin::default(),
            engines: ExternalStringMap::default(),
            non_optional_peer_dependencies_start: 0,
            _padding_before_man_dir: [0; 4],
            man_dir: ExternalString::default(),
            tarball_url: ExternalString::default(),
            unpacked_size: 0,
            file_count: 0,
            os: OperatingSystem::ALL,
            cpu: Architecture::ALL,
            libc: Libc::NONE,
            has_install_script: false,
            _padding_tail: [0; 2],
            publish_timestamp_ms: 0.0,
        }
    }
}

const _: () = assert!(
    size_of::<PackageVersion>() == 240,
    "Npm.PackageVersion layout drifted from bun spec (expected 240 bytes)"
);

const _: () = {
    // gap between `integrity` (size 65, align 1) and `dependencies` (align 4 → 68)
    assert!(
        offset_of!(PackageVersion, _padding_after_integrity)
            == offset_of!(PackageVersion, integrity) + size_of::<Integrity>()
    );
    assert!(
        offset_of!(PackageVersion, dependencies)
            == offset_of!(PackageVersion, _padding_after_integrity) + 3
    );
    // gap between `non_optional_peer_dependencies_start` (ends at 180) and `man_dir` (align 8 → 184)
    assert!(
        offset_of!(PackageVersion, _padding_before_man_dir)
            == offset_of!(PackageVersion, non_optional_peer_dependencies_start) + size_of::<u32>()
    );
    assert!(
        offset_of!(PackageVersion, man_dir)
            == offset_of!(PackageVersion, _padding_before_man_dir) + 4
    );
    // gap between `has_install_script` (ends at 230) and `publish_timestamp_ms` (align 8 → 232)
    assert!(
        offset_of!(PackageVersion, _padding_tail)
            == offset_of!(PackageVersion, has_install_script) + size_of::<bool>()
    );
    assert!(
        offset_of!(PackageVersion, publish_timestamp_ms)
            == offset_of!(PackageVersion, _padding_tail) + 2
    );
    // anchor a few absolute offsets to catch drift in the middle of the struct
    assert!(offset_of!(PackageVersion, peer_dependencies) == 100);
    assert!(offset_of!(PackageVersion, man_dir) == 184);
    assert!(offset_of!(PackageVersion, publish_timestamp_ms) == 232);
};

// ──────────────────────────────────────────────────────────────────────────
// NpmPackage (120 B) — src/install/npm.rs:812
// ──────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct NpmPackage {
    pub last_modified: SemverString,
    pub etag: SemverString,
    pub modified: SemverString,
    pub public_max_age: u32,
    pub _padding_after_max_age: [u8; 4],
    pub name: ExternalString,
    pub releases: ExternVersionMap,
    pub prereleases: ExternVersionMap,
    pub dist_tags: DistTagMap,
    pub versions_buf: VersionSlice,
    pub string_lists_buf: ExternalStringList,
    pub has_extended_manifest: bool,
    pub _padding_tail: [u8; 7],
}

const _: () = {
    // gap between `public_max_age` (ends at 28) and `name` (align 8 → 32)
    assert!(
        offset_of!(NpmPackage, _padding_after_max_age)
            == offset_of!(NpmPackage, public_max_age) + size_of::<u32>()
    );
    assert!(offset_of!(NpmPackage, name) == offset_of!(NpmPackage, _padding_after_max_age) + 4);
    // tail gap after `has_extended_manifest` (bool, at 112) → struct end (120)
    assert!(
        offset_of!(NpmPackage, _padding_tail)
            == offset_of!(NpmPackage, has_extended_manifest) + size_of::<bool>()
    );
    assert!(offset_of!(NpmPackage, _padding_tail) + 7 == size_of::<NpmPackage>());
    assert!(size_of::<NpmPackage>() == 120);
};
