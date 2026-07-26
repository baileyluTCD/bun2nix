// Vendored verbatim from bun `src/wyhash/lib.rs` @ commit 5621c5de90.
// Kept byte-identical to upstream; do not refactor or reformat.
// Note: `#![feature(hasher_prefixfree_extras)]` from the original crate root
// is omitted here — it is a crate-level attribute (inapplicable in a module
// file) and is not required by any item in lines 1–387 that we vendor.

//
// `Wyhash11` is an older wyhash variant (32-byte rounds, 5 primes).
//
// `Wyhash` is the current wyhash (final4 variant, 48-byte rounds, 4 secrets) —
// used by RuntimeTranspilerCache, `bun.hash()`, router.
//
// THESE ARE DIFFERENT ALGORITHMS. They produce different outputs for the same input.
//

// ════════════════════════════════════════════════════════════════════════════
// Wyhash11 (legacy, 32-byte rounds, 5 primes)
// ════════════════════════════════════════════════════════════════════════════

const PRIMES: [u64; 5] = [
    0xa0761d6478bd642f,
    0xe7037ed1a0b428db,
    0x8ebc6af09c88c6e3,
    0x589965cc75374cc3,
    0x1d8e4e27c47d124f,
];

// A single unaligned load. Mirrors the `Wyhash::read4`/`read8` treatment
// below (~lib.rs:600): the per-byte `[data[0], data[1], ...]` spelling left a
// cmp/je-to-panic ladder per byte in `WyhashStateless::final_`/`round`, and
// `final_` is the `StringHashMap` short-key hash — hit on every parser /
// resolver / module-registry HashMap probe during startup (identifier/path
// keys are <32B so `aligned_len == 0` and the whole key goes through `final_`).
// Every caller proves the length first (the `1..=31` arms of `final_` slice
// `rem_key = &b[0..rem_len]` with `rem_len` == the match scrutinee; `round`
// takes a `&[u8]` it `debug_assert!`s == 32), so a single unaligned load is sound.
#[inline(always)]
fn read_bytes<const BYTES: u8>(data: &[u8]) -> u64 {
    debug_assert!(data.len() >= usize::from(BYTES));
    // Rust cannot mint an integer type from a const generic; dispatch on the only values used.
    match BYTES {
        1 => u64::from(data[0]),
        2 => u64::from(u16::from_le_bytes([data[0], data[1]])),
        // SAFETY: `data.len() >= BYTES == 4` (asserted above; every caller
        // proves it). `read_unaligned` imposes no alignment requirement.
        4 => u64::from(u32::from_le(unsafe {
            core::ptr::read_unaligned(data.as_ptr().cast::<u32>())
        })),
        // SAFETY: `data.len() >= BYTES == 8` (asserted above; every caller
        // proves it). `read_unaligned` imposes no alignment requirement.
        8 => u64::from_le(unsafe { core::ptr::read_unaligned(data.as_ptr().cast::<u64>()) }),
        _ => unreachable!(),
    }
}

#[inline(always)]
fn read_8bytes_swapped(data: &[u8]) -> u64 {
    (read_bytes::<4>(data) << 32) | read_bytes::<4>(&data[4..])
}

// `#[inline(always)]`, not just `#[inline]`: these 4 helpers (`mum` + the two
// `mix*` wrappers, alongside the already-`always` `read_8bytes_swapped`) are the
// entire body of the short-key (`0..=16` byte) hash that `StringHashMap` /
// hashbrown runs per identifier / keyword / module-path key while parsing every
// module. `#[inline]` (hint) was being declined across the bun_wyhash →
// bun_collections / bun_js_parser crate boundary, leaving an out-of-line `mum`
// (and a `call` per mix step) inside the hashbrown probe loop. Force the 4 mix
// steps to fold in with no call/ret — same reasoning the surrounding `final_*`
// helpers are `#[inline(always)]` / `#[cold]`-split for.
#[inline(always)]
fn mum(a: u64, b: u64) -> u64 {
    let mut r = (a as u128) * (b as u128);
    r = (r >> 64) ^ r;
    r as u64
}

#[inline(always)]
fn mix0(a: u64, b: u64, seed: u64) -> u64 {
    mum(a ^ seed ^ PRIMES[0], b ^ seed ^ PRIMES[1])
}

#[inline(always)]
fn mix1(a: u64, b: u64, seed: u64) -> u64 {
    mum(a ^ seed ^ PRIMES[2], b ^ seed ^ PRIMES[3])
}

/// Cold tail of [`WyhashStateless::final_`] — the `17..=31`-byte remainder
/// arms. Split out (and marked `#[cold] #[inline(never)]`) so the common
/// short-key path (`0..=16`, which covers virtually every identifier/path key
/// the parser/resolver/module-registry hash through `StringHashMap`) stays
/// small enough to inline into the hashbrown probe loop. `key.len()` is in
/// `17..=31`; `seed` is the running `WyhashStateless::seed`.
#[cold]
#[inline(never)]
fn final_long(seed: u64, key: &[u8]) -> u64 {
    debug_assert!((17..32).contains(&key.len()));

    let head = mix0(
        read_8bytes_swapped(key),
        read_8bytes_swapped(&key[8..]),
        seed,
    );
    let tail = match key.len() {
        17 => mix1(read_bytes::<1>(&key[16..]), PRIMES[4], seed),
        18 => mix1(read_bytes::<2>(&key[16..]), PRIMES[4], seed),
        19 => mix1(
            (read_bytes::<2>(&key[16..]) << 8) | read_bytes::<1>(&key[18..]),
            PRIMES[4],
            seed,
        ),
        20 => mix1(read_bytes::<4>(&key[16..]), PRIMES[4], seed),
        21 => mix1(
            (read_bytes::<4>(&key[16..]) << 8) | read_bytes::<1>(&key[20..]),
            PRIMES[4],
            seed,
        ),
        22 => mix1(
            (read_bytes::<4>(&key[16..]) << 16) | read_bytes::<2>(&key[20..]),
            PRIMES[4],
            seed,
        ),
        23 => mix1(
            (read_bytes::<4>(&key[16..]) << 24)
                | (read_bytes::<2>(&key[20..]) << 8)
                | read_bytes::<1>(&key[22..]),
            PRIMES[4],
            seed,
        ),
        24 => mix1(read_8bytes_swapped(&key[16..]), PRIMES[4], seed),
        25 => mix1(
            read_8bytes_swapped(&key[16..]),
            read_bytes::<1>(&key[24..]),
            seed,
        ),
        26 => mix1(
            read_8bytes_swapped(&key[16..]),
            read_bytes::<2>(&key[24..]),
            seed,
        ),
        27 => mix1(
            read_8bytes_swapped(&key[16..]),
            (read_bytes::<2>(&key[24..]) << 8) | read_bytes::<1>(&key[26..]),
            seed,
        ),
        28 => mix1(
            read_8bytes_swapped(&key[16..]),
            read_bytes::<4>(&key[24..]),
            seed,
        ),
        29 => mix1(
            read_8bytes_swapped(&key[16..]),
            (read_bytes::<4>(&key[24..]) << 8) | read_bytes::<1>(&key[28..]),
            seed,
        ),
        30 => mix1(
            read_8bytes_swapped(&key[16..]),
            (read_bytes::<4>(&key[24..]) << 16) | read_bytes::<2>(&key[28..]),
            seed,
        ),
        31 => mix1(
            read_8bytes_swapped(&key[16..]),
            (read_bytes::<4>(&key[24..]) << 24)
                | (read_bytes::<2>(&key[28..]) << 8)
                | read_bytes::<1>(&key[30..]),
            seed,
        ),
        _ => unreachable!(),
    };
    head ^ tail
}

// Wyhash version which does not store internal state for handling partial buffers.
// This is needed so that we can maximize the speed for the short key case, which will
// use the non-iterative api which the public Wyhash exposes.
#[derive(Clone, Copy)]
struct WyhashStateless {
    seed: u64,
    msg_len: usize,
}

impl WyhashStateless {
    #[inline(always)]
    pub(crate) fn init(seed: u64) -> WyhashStateless {
        WyhashStateless { seed, msg_len: 0 }
    }

    #[inline(always)]
    fn round(&mut self, b: &[u8]) {
        debug_assert!(b.len() == 32);

        self.seed = mix0(
            read_bytes::<8>(&b[0..]),
            read_bytes::<8>(&b[8..]),
            self.seed,
        ) ^ mix1(
            read_bytes::<8>(&b[16..]),
            read_bytes::<8>(&b[24..]),
            self.seed,
        );
    }

    #[inline(always)]
    pub(crate) fn update(&mut self, b: &[u8]) {
        debug_assert!(b.len().is_multiple_of(32));

        let mut off: usize = 0;
        while off < b.len() {
            self.round(&b[off..off + 32]);
            off += 32;
        }

        self.msg_len += b.len();
    }

    // `final_` is the `StringHashMap` short-key hash (every parser / resolver /
    // module-registry probe during startup). `#[inline(always)]` alone wasn't
    // enough — the 31-arm length switch is large enough that LLVM still emitted
    // an out-of-line `final_` symbol and `call`ed it. Identifier/path keys are
    // almost always <17B, so split the cold `17..=31` tail into a separate
    // `#[cold] #[inline(never)] final_long`, leaving `final_` with just the
    // `0..=16` arms — small enough to inline cleanly into every hashbrown probe.
    #[inline(always)]
    pub(crate) fn final_(&mut self, b: &[u8]) -> u64 {
        debug_assert!(b.len() < 32);

        let seed = self.seed;
        let rem_len = b.len();
        let rem_key = &b[0..rem_len];

        self.seed = match rem_len {
            0 => seed,
            1 => mix0(read_bytes::<1>(rem_key), PRIMES[4], seed),
            2 => mix0(read_bytes::<2>(rem_key), PRIMES[4], seed),
            3 => mix0(
                (read_bytes::<2>(rem_key) << 8) | read_bytes::<1>(&rem_key[2..]),
                PRIMES[4],
                seed,
            ),
            4 => mix0(read_bytes::<4>(rem_key), PRIMES[4], seed),
            5 => mix0(
                (read_bytes::<4>(rem_key) << 8) | read_bytes::<1>(&rem_key[4..]),
                PRIMES[4],
                seed,
            ),
            6 => mix0(
                (read_bytes::<4>(rem_key) << 16) | read_bytes::<2>(&rem_key[4..]),
                PRIMES[4],
                seed,
            ),
            7 => mix0(
                (read_bytes::<4>(rem_key) << 24)
                    | (read_bytes::<2>(&rem_key[4..]) << 8)
                    | read_bytes::<1>(&rem_key[6..]),
                PRIMES[4],
                seed,
            ),
            8 => mix0(read_8bytes_swapped(rem_key), PRIMES[4], seed),
            9 => mix0(
                read_8bytes_swapped(rem_key),
                read_bytes::<1>(&rem_key[8..]),
                seed,
            ),
            10 => mix0(
                read_8bytes_swapped(rem_key),
                read_bytes::<2>(&rem_key[8..]),
                seed,
            ),
            11 => mix0(
                read_8bytes_swapped(rem_key),
                (read_bytes::<2>(&rem_key[8..]) << 8) | read_bytes::<1>(&rem_key[10..]),
                seed,
            ),
            12 => mix0(
                read_8bytes_swapped(rem_key),
                read_bytes::<4>(&rem_key[8..]),
                seed,
            ),
            13 => mix0(
                read_8bytes_swapped(rem_key),
                (read_bytes::<4>(&rem_key[8..]) << 8) | read_bytes::<1>(&rem_key[12..]),
                seed,
            ),
            14 => mix0(
                read_8bytes_swapped(rem_key),
                (read_bytes::<4>(&rem_key[8..]) << 16) | read_bytes::<2>(&rem_key[12..]),
                seed,
            ),
            15 => mix0(
                read_8bytes_swapped(rem_key),
                (read_bytes::<4>(&rem_key[8..]) << 24)
                    | (read_bytes::<2>(&rem_key[12..]) << 8)
                    | read_bytes::<1>(&rem_key[14..]),
                seed,
            ),
            16 => mix0(
                read_8bytes_swapped(rem_key),
                read_8bytes_swapped(&rem_key[8..]),
                seed,
            ),
            // Keys ≥17B are rare among identifier/path keys; keep this tail out
            // of line so the `0..=16` arms above inline into every caller.
            _ => final_long(seed, rem_key),
        };

        self.msg_len += b.len();
        mum(self.seed ^ (self.msg_len as u64), PRIMES[4])
    }

    // perf on build/create-next showed `WyhashStateless::hash` out-lined as a
    // standalone symbol (91 self-samples) and `call`ed from
    // `find_symbol_with_record_usage` and every `StringHashMap`/hashbrown probe
    // — `#[inline]` (hint) was being declined across the bun_wyhash →
    // bun_js_parser/bun_collections crate boundary; force it.
    #[inline(always)]
    pub(crate) fn hash(seed: u64, input: &[u8]) -> u64 {
        let aligned_len = input.len() - (input.len() % 32);

        let mut c = WyhashStateless::init(seed);
        c.update(&input[0..aligned_len]);
        c.final_(&input[aligned_len..])
    }
}

/// Fast non-cryptographic 64bit hash function.
/// See https://github.com/wangyi-fudan/wyhash
pub struct Wyhash11 {
    state: WyhashStateless,

    buf: [u8; 32],
    buf_len: usize,
}

impl Wyhash11 {
    #[inline]
    pub fn init(seed: u64) -> Wyhash11 {
        Wyhash11 {
            state: WyhashStateless::init(seed),
            buf: [0; 32],
            buf_len: 0,
        }
    }

    #[inline]
    pub fn update(&mut self, b: &[u8]) {
        let mut off: usize = 0;

        if self.buf_len != 0 && self.buf_len + b.len() >= 32 {
            off += 32 - self.buf_len;
            self.buf[self.buf_len..self.buf_len + off].copy_from_slice(&b[0..off]);
            self.state.update(&self.buf[0..]);
            self.buf_len = 0;
        }

        let remain_len = b.len() - off;
        let aligned_len = remain_len - (remain_len % 32);
        self.state.update(&b[off..off + aligned_len]);

        let tail = &b[off + aligned_len..];
        self.buf[self.buf_len..self.buf_len + tail.len()].copy_from_slice(tail);
        self.buf_len += usize::from(u8::try_from(tail.len()).expect("int cast"));
    }

    // Force-inline so no out-of-line copy of `WyhashStateless::final_`'s
    // length-switch survives on the `Hasher`-driven streaming path.
    #[inline(always)]
    pub fn final_(&mut self) -> u64 {
        let rem_key = &self.buf[0..self.buf_len];

        self.state.final_(rem_key)
    }

    #[inline(always)]
    pub fn hash(seed: u64, input: &[u8]) -> u64 {
        WyhashStateless::hash(seed, input)
    }
}

// Allow `Wyhash11` to be used with `core::hash::Hash::hash` (e.g., as the
// state for std/HashMap-style hashing).
impl core::hash::Hasher for Wyhash11 {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        self.update(bytes);
    }
    #[inline]
    fn finish(&self) -> u64 {
        // `final_` mutates `state`; clone so `Hasher::finish(&self)` stays
        // semantically pure (matches std contract).
        let mut s = self.state;
        s.final_(&self.buf[0..self.buf_len])
    }
}

/// bun's legacy wyhash (32-byte rounds, 5 primes) — the hash bun uses for
/// on-disk cache keys and `.npm` manifest filenames. Vendored verbatim from
/// bun `src/wyhash/lib.rs`; keep byte-identical to upstream.
pub fn wyhash11(seed: u64, input: &[u8]) -> u64 {
    Wyhash11::hash(seed, input)
}

#[cfg(test)]
mod tests {
    use super::wyhash11;

    // These hex values are the lower-16 hex of wyhash11(0, name), taken from
    // the Zig creator's golden cache-name tests (programs/cache-entry-creator
    // src/main.zig "cachedNpmPackageFolderPrintBasename"): the build-metadata
    // component of "react@1.2.3+build.123" hashes "build.123" -> F48F05ED5AABC3A0.
    #[test]
    fn matches_bun_known_vectors() {
        assert_eq!(
            format!("{:016X}", wyhash11(0, b"build.123")),
            "F48F05ED5AABC3A0"
        );
    }
}
