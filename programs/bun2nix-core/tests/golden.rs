use bun2nix_core::manifest::{
    self, Dep, SingleVersionInput, build, build_single_version, meta, read, resolve_str, serialize,
};

/// The default-registry url_hash bun stores in every `.npm` header must equal
/// `wyhash11(0, "https://registry.npmjs.org")`. This pins both the wyhash port
/// and the exact bytes hashed.
#[test]
fn default_url_hash_matches_header_constant() {
    assert_eq!(
        manifest::default_url_hash(),
        manifest::DEFAULT_URL_HASH,
        "wyhash11 of the registry URL must match the constant read from real .npm headers"
    );
}

/// `@neoconfetti/svelte`'s manifest filename is `wyhash11(0, name)` hex.
#[test]
fn neoconfetti_filename() {
    let f = manifest::manifest_file_name("@neoconfetti/svelte");
    eprintln!("@neoconfetti/svelte -> {f}");
    assert!(f.ends_with(".npm"));
}

fn neoconfetti_input() -> SingleVersionInput<'static> {
    SingleVersionInput {
        name: "@neoconfetti/svelte",
        version: (2, 2, 2),
        sha512: None,
        dependencies: vec![],
        optional_dependencies: vec![],
        peer_dependencies: vec![Dep {
            name: "svelte".to_string(),
            range: "^3.0.0 || ^4.0.0 || ^5.0.0".to_string(),
        }],
    }
}

/// Round-trip: serialize a hand-built manifest, read it back, and assert every
/// field bun relies on for peer-dependency resolution survives intact. This is
/// the validation gate (byte-golden against real bun output is not reproducible
/// for full multi-version manifests — see the report).
#[test]
fn neoconfetti_round_trip() {
    let m = build_single_version(&neoconfetti_input());
    let mut out = Vec::new();
    serialize::write(
        &m,
        manifest::DEFAULT_URL_HASH,
        manifest::DEFAULT_REGISTRY_HREF_LEN,
        &mut out,
    )
    .unwrap();

    // Header framing.
    assert_eq!(&out[..serialize::HEADER.len()], serialize::HEADER);

    let rd = read(&out).expect("header must parse");
    assert_eq!(rd.url_hash, manifest::DEFAULT_URL_HASH);
    assert_eq!(rd.href_len, manifest::DEFAULT_REGISTRY_HREF_LEN);

    // Package name resolves correctly.
    assert_eq!(
        resolve_str(&rd.pkg.name, &rd.string_buf),
        b"@neoconfetti/svelte"
    );

    // One release version, 2.2.2.
    assert_eq!(rd.versions.len(), 1);
    let v = rd.versions[0];
    assert_eq!((v.major, v.minor, v.patch), (2, 2, 2));
    assert_eq!(rd.pkg.releases.keys.len, 1);
    assert_eq!(rd.pkg.releases.values.len, 1);

    // The single PackageVersion has exactly one peer dependency: svelte.
    assert_eq!(rd.package_versions.len(), 1);
    let pv = rd.package_versions[0];
    let peer = pv.peer_dependencies;
    assert_eq!(peer.name.len, 1);
    assert_eq!(peer.value.len, 1);

    let dep_name = &rd.external_strings[peer.name.off as usize];
    let dep_range = &rd.external_strings_for_versions[peer.value.off as usize];
    assert_eq!(resolve_str(dep_name, &rd.string_buf), b"svelte");
    assert_eq!(
        resolve_str(dep_range, &rd.string_buf),
        b"^3.0.0 || ^4.0.0 || ^5.0.0"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// Task 4 builder tests (multi-version manifest)
// ──────────────────────────────────────────────────────────────────────────

/// Construct a `PackageMeta` for @neoconfetti/svelte 2.2.2 from the fixture.
fn neoconfetti_meta() -> meta::PackageMeta {
    let fixture =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/neoconfetti.json"))
            .expect("neoconfetti.json fixture missing");
    serde_json::from_str(&fixture).expect("failed to parse neoconfetti.json")
}

/// Single-version round-trip via the general builder: load the neoconfetti
/// fixture, call `build_manifest`, serialise, deserialise, assert fields.
///
/// Verifies: name, tarball_url, peer_dependencies, public_max_age=u32::MAX.
#[test]
fn builder_round_trips() {
    let pkg = neoconfetti_meta();
    let built = build::build_manifest(&pkg);

    let mut out = Vec::new();
    let url_hash = manifest::default_url_hash();
    let href_len = manifest::DEFAULT_REGISTRY_URL.trim_end_matches('/').len() as u64;
    serialize::write(&built, url_hash, href_len, &mut out).unwrap();

    let parsed = read(&out).expect("header must parse");

    assert_eq!(parsed.name(), b"@neoconfetti/svelte");

    let v = parsed
        .find_version("2.2.2")
        .expect("version 2.2.2 must be present");
    assert!(
        v.peer_dependencies().contains_key("svelte"),
        "svelte peer dep must survive round-trip"
    );
    assert_eq!(
        v.tarball_url(),
        pkg.versions[0].tarball_url,
        "tarball_url must survive round-trip"
    );
    assert_eq!(parsed.public_max_age(), u32::MAX, "manifest must never expire");
}

/// Construct a `PackageMeta` for `ms` with two versions from the fixture.
fn ms_meta() -> meta::PackageMeta {
    let fixture =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ms.json"))
            .expect("ms.json fixture missing");
    serde_json::from_str(&fixture).expect("failed to parse ms.json")
}

/// Multi-version round-trip: load `ms` with versions 2.0.0 and 2.1.2, build a
/// manifest, serialise, deserialise, assert BOTH versions are present and their
/// tarball URLs survive the round-trip.
#[test]
fn builder_multi_version() {
    let pkg = ms_meta();
    assert_eq!(pkg.versions.len(), 2, "fixture must have two versions");

    let built = build::build_manifest(&pkg);

    let mut out = Vec::new();
    let url_hash = manifest::default_url_hash();
    let href_len = manifest::DEFAULT_REGISTRY_URL.trim_end_matches('/').len() as u64;
    serialize::write(&built, url_hash, href_len, &mut out).unwrap();

    let parsed = read(&out).expect("header must parse");

    assert_eq!(parsed.name(), b"ms");
    assert_eq!(parsed.public_max_age(), u32::MAX, "manifest must never expire");

    // Both versions must be findable and their tarball URLs must survive.
    for vm in &pkg.versions {
        let v = parsed
            .find_version(&vm.version)
            .unwrap_or_else(|| panic!("version {} must be present", vm.version));
        assert_eq!(
            v.tarball_url(),
            vm.tarball_url,
            "tarball_url for version {} must survive round-trip",
            vm.version
        );
    }
}

/// Round-trip test that validates the bun ABI ordering for peer dependencies:
/// optional peers must occupy indices `[0, non_optional_peer_dependencies_start)`
/// and non-optional peers must occupy `[start, len)`.  The field itself must
/// equal the number of optional peers (not the number of non-optional peers).
///
/// This test FAILS against the old (inverted) code that placed non-optional
/// peers first and stored the non-optional count in the field.
#[test]
fn peer_dep_optional_ordering_round_trip() {
    use std::collections::BTreeMap;

    let pkg = meta::PackageMeta {
        name: "fake-pkg".to_string(),
        versions: vec![meta::VersionMeta {
            version: "1.0.0".to_string(),
            tarball_url: "https://registry.npmjs.org/fake-pkg/-/fake-pkg-1.0.0.tgz".to_string(),
            integrity: String::new(),
            dependencies: BTreeMap::new(),
            optional_dependencies: BTreeMap::new(),
            peer_dependencies: {
                let mut m = BTreeMap::new();
                // BTreeMap iterates alphabetically, so "optional-peer" < "required-peer".
                // After the fix, optional-peer must appear first regardless of
                // alphabetical order.
                m.insert("optional-peer".to_string(), "^1.0.0".to_string());
                m.insert("required-peer".to_string(), "^2.0.0".to_string());
                m
            },
            optional_peers: vec!["optional-peer".to_string()],
            bin: BTreeMap::new(),
            os: vec![],
            cpu: vec![],
            has_install_script: false,
        }],
    };

    let built = build::build_manifest(&pkg);
    let mut out = Vec::new();
    serialize::write(
        &built,
        manifest::DEFAULT_URL_HASH,
        manifest::DEFAULT_REGISTRY_HREF_LEN,
        &mut out,
    )
    .unwrap();

    let parsed = read(&out).expect("manifest must parse");
    let v = parsed.find_version("1.0.0").expect("version 1.0.0 must be present");

    let total = v.pv.peer_dependencies.name.len as usize;
    let start = v.pv.non_optional_peer_dependencies_start as usize;

    assert_eq!(total, 2, "expected 2 peer deps total");
    // `non_optional_peer_dependencies_start` == count of optional peers == 1.
    assert_eq!(
        start, 1,
        "non_optional_peer_dependencies_start must equal the number of optional peers (1), \
         not the number of non-optional peers"
    );

    // Indices [0, start) must be optional peers.
    for i in 0..start {
        let name = v.peer_dep_name_at(i);
        assert_eq!(
            name, "optional-peer",
            "index {i} (< start={start}) must be an optional peer, got {name:?}"
        );
    }

    // Indices [start, total) must be non-optional peers.
    for i in start..total {
        let name = v.peer_dep_name_at(i);
        assert_eq!(
            name, "required-peer",
            "index {i} (>= start={start}) must be a non-optional (required) peer, got {name:?}"
        );
    }
}

/// Tripwire: assert that the manifest cache format version string is still
/// `bun-npm-manifest-cache-v0.0.7`.
///
/// If bun bumps its manifest cache format, this test (and the functional
/// round-trip tests above) will fail.  To re-port:
///   1. Diff bun's `src/install/npm.rs` against the new version.
///   2. Update the layout structs in `bun2nix-core/src/manifest/layout.rs`
///      and the serializer in `serialize.rs`.
///   3. Bump the version string in `serialize::HEADER`.
///   4. Update the string below to match.
#[test]
fn manifest_format_version_is_pinned() {
    assert!(
        serialize::HEADER.ends_with(b"bun-npm-manifest-cache-v0.0.7\n"),
        "bun manifest cache format version changed — see comment above for re-port procedure"
    );
}

/// Emit the generated `.npm` to a path given by the `EMIT_NPM` env var, so the
/// functional (offline-install) test can drop it into a bun cache. Skipped when
/// the env var is unset.
#[test]
fn emit_neoconfetti_npm() {
    let Ok(path) = std::env::var("EMIT_NPM") else {
        return;
    };
    let m = build_single_version(&neoconfetti_input());
    let mut out = Vec::new();
    serialize::write(
        &m,
        manifest::DEFAULT_URL_HASH,
        manifest::DEFAULT_REGISTRY_HREF_LEN,
        &mut out,
    )
    .unwrap();
    std::fs::write(&path, &out).unwrap();
    eprintln!("wrote {} bytes to {path}", out.len());
}
