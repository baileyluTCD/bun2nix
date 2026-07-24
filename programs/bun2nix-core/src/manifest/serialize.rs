//! Serializer for bun's `bun-npm-manifest-cache-v0.0.7` `.npm` files.
//!
//! Mirrors `PackageManifest` + `Serializer::write` / `write_array` / `Aligner`
//! from `src/install/npm.rs` (~865, ~926-1025) and `src/install/lib.rs` (~1040).
//! The on-disk framing is:
//!   1. 49-byte ASCII header
//!   2. `u64` LE registry url_hash
//!   3. `u64` LE registry href length (without trailing slash)
//!   4. `pkg: NpmPackage` (aligned to align_of::<NpmPackage>())
//!   5. seven arrays, each: `u64` LE byte-length, alignment padding, raw bytes.
//!      (a zero-length array is just the `u64` 0, with no padding.)

use std::io::{self, Write};

use super::layout::{ExternalString, NpmPackage, PackageVersion, SemverVersion};

/// `#!/usr/bin/env bun\nbun-npm-manifest-cache-v0.0.7\n` — exactly 49 bytes.
/// (The length is not serialized, so it must be fixed.)
pub const HEADER: &[u8] = b"#!/usr/bin/env bun\nbun-npm-manifest-cache-v0.0.7\n";

pub const HEADER_LEN_ASSERT: () = assert!(HEADER.len() == 49);

/// In-memory manifest mirroring `npm.rs:865`'s `PackageManifest`.
///
/// `pkg` holds the fixed-size header struct; the seven boxed slices are the
/// flat backing buffers that the slices/offsets inside `pkg` (and inside each
/// `PackageVersion`) index into.
#[derive(Default, Clone)]
pub struct PackageManifest {
    pub pkg: NpmPackage,
    pub string_buf: Box<[u8]>,
    pub versions: Box<[SemverVersion]>,
    pub external_strings: Box<[ExternalString]>,
    /// Backing buffer for dependency *values* (version ranges) and version tag
    /// strings. Kept separate so contiguous identical versions can be deduped.
    pub external_strings_for_versions: Box<[ExternalString]>,
    pub package_versions: Box<[PackageVersion]>,
    pub extern_strings_bin_entries: Box<[ExternalString]>,
    pub bundled_deps_buf: Box<[u64]>,
}

/// Number of zero bytes needed to advance `pos` to the next multiple of `align`.
/// Mirrors `Aligner::skip_amount_with_align`.
#[inline]
fn skip_amount(align: usize, pos: u64) -> usize {
    (pos as usize).next_multiple_of(align) - (pos as usize)
}

/// Write alignment padding (zero bytes) for `align`, returning the count.
/// Mirrors `Aligner::write`.
fn write_alignment(w: &mut impl Write, align: usize, pos: u64) -> io::Result<usize> {
    let to_write = skip_amount(align, pos);
    if to_write > 0 {
        // bun repeats from a 144-byte zero buffer; equivalent to writing zeros.
        const ZEROS: [u8; 144] = [0u8; 144];
        let mut remaining = to_write;
        while remaining > 0 {
            let n = remaining.min(ZEROS.len());
            w.write_all(&ZEROS[..n])?;
            remaining -= n;
        }
    }
    Ok(to_write)
}

/// Reinterpret a `&[T]` of POD as raw bytes (the structs have explicit padding,
/// so every byte is initialized).
fn as_bytes<T: Copy>(array: &[T]) -> &[u8] {
    // SAFETY: T is a `#[repr(C)]` POD with no implicit padding (guarded by the
    // layout asserts in `layout.rs`), so all bytes are initialized.
    unsafe { std::slice::from_raw_parts(array.as_ptr().cast::<u8>(), std::mem::size_of_val(array)) }
}

/// Mirror of `Serializer::write_array`.
fn write_array<T: Copy>(w: &mut impl Write, array: &[T], pos: &mut u64) -> io::Result<()> {
    let bytes = as_bytes(array);
    if bytes.is_empty() {
        w.write_all(&0u64.to_le_bytes())?;
        *pos += 8;
        return Ok(());
    }
    w.write_all(&(bytes.len() as u64).to_le_bytes())?;
    *pos += 8;
    *pos += write_alignment(w, std::mem::align_of::<T>(), *pos)? as u64;
    w.write_all(bytes)?;
    *pos += bytes.len() as u64;
    Ok(())
}

/// Serialize `m` into bun's `.npm` binary format. `url_hash` and
/// `registry_href_len` are the registry scope fields (for the default registry,
/// `super::DEFAULT_URL_HASH` and `26`).
pub fn write(
    m: &PackageManifest,
    url_hash: u64,
    registry_href_len: u64,
    w: &mut impl Write,
) -> io::Result<()> {
    let mut pos: u64 = 0;

    w.write_all(HEADER)?;
    pos += HEADER.len() as u64;

    w.write_all(&url_hash.to_le_bytes())?;
    w.write_all(&registry_href_len.to_le_bytes())?;
    pos += 16;

    // pkg (aligned to align_of::<NpmPackage>())
    {
        let bytes = as_bytes(std::slice::from_ref(&m.pkg));
        pos += write_alignment(w, std::mem::align_of::<NpmPackage>(), pos)? as u64;
        w.write_all(bytes)?;
        pos += bytes.len() as u64;
    }

    write_array(w, &m.string_buf, &mut pos)?;
    write_array(w, &m.versions, &mut pos)?;
    write_array(w, &m.external_strings, &mut pos)?;
    write_array(w, &m.external_strings_for_versions, &mut pos)?;
    write_array(w, &m.package_versions, &mut pos)?;
    write_array(w, &m.extern_strings_bin_entries, &mut pos)?;
    write_array(w, &m.bundled_deps_buf, &mut pos)?;

    Ok(())
}
