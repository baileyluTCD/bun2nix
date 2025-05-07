use std::fs;

#[test]
fn test_parse_git_deps_lockfile() {
    let lockfile = fs::read_to_string("./nix/templates/git-deps/bun.lock")
        .expect("Could not find git deps lockfile for integration test");

    let parsed = bun2nix::convert_lockfile_to_nix_expression(lockfile).unwrap();

    let correct_nix = fs::read_to_string("./nix/templates/git-deps/bun.nix").unwrap();

    assert_eq!(parsed.trim(), correct_nix.trim());
}
