use bun2nix::{package::FetchMany, Lockfile, Package};

#[tokio::test]
async fn test_prefetch_packages() {
    let lockfile = r#"
        {
          "lockfileVersion": 1,
          "workspaces": {
            "": {
              "name": "examples",
              "devDependencies": {
                "@types/bun": "latest",
              },
              "peerDependencies": {
                "typescript": "^5",
              },
            },
          },
          "packages": {
            "@types/bun": ["@types/bun@1.2.4", "", { "dependencies": { "bun-types": "1.2.4" } }, "sha512-QtuV5OMR8/rdKJs213iwXDpfVvnskPXY/S0ZiFbsTjQZycuqPbMW8Gf/XhLfwE5njW8sxI2WjISURXPlHypMFA=="],

            "@types/node": ["@types/node@22.13.5", "", { "dependencies": { "undici-types": "~6.20.0" } }, "sha512-+lTU0PxZXn0Dr1NBtC7Y8cR21AJr87dLLU953CWA6pMxxv/UDc7jYAY90upcrie1nRcD6XNG5HOYEDtgW5TxAg=="],

            "@types/ws": ["@types/ws@8.5.14", "", { "dependencies": { "@types/node": "*" } }, "sha512-bd/YFLW+URhBzMXurx7lWByOu+xzU9+kb3RboOteXYDfW+tr+JZa99OyNmPINEGB/ahzKrEuc8rcv4gnpJmxTw=="],

            "bun-types": ["bun-types@1.2.4", "", { "dependencies": { "@types/node": "*", "@types/ws": "~8.5.10" } }, "sha512-nDPymR207ZZEoWD4AavvEaa/KZe/qlrbMSchqpQwovPZCKc7pwMoENjEtHgMKaAjJhy+x6vfqSBA1QU3bJgs0Q=="],

            "typescript": ["typescript@5.7.3", "", { "bin": { "tsc": "bin/tsc", "tsserver": "bin/tsserver" } }, "sha512-84MVSjMEHP+FQRPy3pX9sTVV/INIex71s9TL2Gm5FG/WG1SqXeKyZ0k7/blY/4FdOzI12CBy1vGc4og/eus0fw=="],

            "undici-types": ["undici-types@6.20.0", "", {}, "sha512-Ny6QZ2Nju20vw1SRHe3d9jVu6gJ+4e3+MMpqu7pqE5HT6WsTSlce++GQmK5UXS8mzV8DSYHrQH+Xrf2jVcuKNg=="],
          }
        }
    "#;

    let value: Lockfile = lockfile.parse().unwrap();

    assert!(value.lockfile_version == 1);
    assert!(value.workspaces[""].name == Some(String::from("examples")));

    assert!(value.packages.contains(&Package {
        name: "@types/bun".to_owned(),
        ..Default::default()
    }));

    let prefetched = value.packages().fetch_many().await.unwrap();

    assert!(!prefetched.is_empty());
}
