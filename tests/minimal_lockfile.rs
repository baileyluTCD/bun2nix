use std::fs;

#[test]
fn test_parse_minimal_lockfile() {
    let lockfile = fs::read_to_string("./examples/minimal/bun.lock")
        .expect("Could not find example lockfile for integration test");

    let parsed = bun2nix::convert_lockfile_to_nix_expression(lockfile);

    println!("parsed: {:#?}", parsed);

    assert!(parsed.is_ok());
}
