mod common;

use common::TestHome;
use serde_json::{json, Value};

const FETCHED_AT: &str = "2026-01-01T00:00:00Z";

fn package(name: &str, registry: &str, version: &str, path: &str) -> Value {
    json!({
        "name": name,
        "version": version,
        "registry": registry,
        "path": path,
        "fetchedAt": FETCHED_AT,
    })
}

fn repo(name: &str, version: &str, path: &str) -> Value {
    json!({
        "name": name,
        "version": version,
        "path": path,
        "fetchedAt": FETCHED_AT,
    })
}

#[test]
fn removes_bare_scoped_npm_package() {
    let home = TestHome::new();
    let node = package(
        "@types/node",
        "npm",
        "20.0.0",
        "repos/github.com/DefinitelyTyped/DefinitelyTyped/20.0.0/types/node",
    );
    let zod = package(
        "zod",
        "npm",
        "3.22.0",
        "repos/github.com/colinhacks/zod/3.22.0",
    );
    home.seed_index(json!({ "packages": [node, zod.clone()] }));
    home.seed_folder("repos/github.com/DefinitelyTyped/DefinitelyTyped/20.0.0/types/node");
    home.seed_folder("repos/github.com/colinhacks/zod/3.22.0");

    let run = home.run(&["remove", "@types/node"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(
        run.stdout.contains("✓ Removed @types/node (npm)"),
        "{run:?}"
    );
    assert_eq!(home.index(), Some(json!({ "packages": [zod] })));
    assert!(!home.exists("repos/github.com/DefinitelyTyped"));
    assert!(home.exists("repos/github.com/colinhacks/zod/3.22.0/MARKER"));
}

#[test]
fn removing_repo_keeps_package_source_in_same_repo_and_deletes_orphans() {
    let home = TestHome::new();
    let zod_pkg = package(
        "zod",
        "npm",
        "3.22.0",
        "repos/github.com/colinhacks/zod/3.22.0",
    );
    let zod_repo = repo(
        "github.com/colinhacks/zod",
        "main",
        "repos/github.com/colinhacks/zod/main",
    );
    home.seed_index(json!({ "packages": [zod_pkg.clone()], "repos": [zod_repo] }));
    home.seed_folder("repos/github.com/colinhacks/zod/3.22.0");
    home.seed_folder("repos/github.com/colinhacks/zod/main");
    home.seed_folder("repos/github.com/colinhacks/zod/old-ref");

    let run = home.run(&["remove", "colinhacks/zod"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(
        run.stdout.contains("✓ Removed github.com/colinhacks/zod"),
        "{run:?}"
    );
    assert_eq!(home.index(), Some(json!({ "packages": [zod_pkg] })));
    assert!(home.exists("repos/github.com/colinhacks/zod/3.22.0/MARKER"));
    assert!(!home.exists("repos/github.com/colinhacks/zod/main"));
    assert!(!home.exists("repos/github.com/colinhacks/zod/old-ref"));
}

#[test]
fn removing_package_deletes_its_version_folder_when_sibling_uses_another_version() {
    let home = TestHome::new();
    let b = package(
        "@scope/b",
        "npm",
        "2.0.0",
        "repos/github.com/acme/mono/2.0.0/packages/b",
    );
    home.seed_index(json!({
        "packages": [
            package("@scope/a", "npm", "1.0.0", "repos/github.com/acme/mono/1.0.0/packages/a"),
            b.clone(),
        ]
    }));
    home.seed_folder("repos/github.com/acme/mono/1.0.0/packages/a");
    home.seed_folder("repos/github.com/acme/mono/2.0.0/packages/b");

    let run = home.run(&["remove", "@scope/a"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert_eq!(home.index(), Some(json!({ "packages": [b] })));
    assert!(!home.exists("repos/github.com/acme/mono/1.0.0"));
    assert!(home.exists("repos/github.com/acme/mono/2.0.0/packages/b/MARKER"));
}

#[test]
fn shared_version_folder_survives_until_last_package_is_removed() {
    let home = TestHome::new();
    let b = package(
        "@scope/b",
        "npm",
        "1.0.0",
        "repos/github.com/acme/mono/1.0.0/packages/b",
    );
    let other = package("other", "npm", "1.0.0", "repos/github.com/other/lib/1.0.0");
    home.seed_index(json!({
        "packages": [
            package("@scope/a", "npm", "1.0.0", "repos/github.com/acme/mono/1.0.0/packages/a"),
            b.clone(),
            other.clone(),
        ]
    }));
    home.seed_folder("repos/github.com/acme/mono/1.0.0/packages/a");
    home.seed_folder("repos/github.com/acme/mono/1.0.0/packages/b");
    home.seed_folder("repos/github.com/other/lib/1.0.0");

    let first = home.run(&["remove", "@scope/a"]);

    assert_eq!(first.code, Some(0), "{first:?}");
    assert_eq!(
        home.index(),
        Some(json!({ "packages": [b, other.clone()] }))
    );
    assert!(home.exists("repos/github.com/acme/mono/1.0.0/packages/a/MARKER"));
    assert!(home.exists("repos/github.com/acme/mono/1.0.0/packages/b/MARKER"));

    let second = home.run(&["remove", "@scope/b"]);

    assert_eq!(second.code, Some(0), "{second:?}");
    assert_eq!(home.index(), Some(json!({ "packages": [other] })));
    assert!(!home.exists("repos/github.com/acme"));
    assert!(home.exists("repos/github.com/other/lib/1.0.0/MARKER"));
}

#[test]
fn removes_several_targets_and_counts_missing_ones() {
    let home = TestHome::new();
    let other = package("other", "npm", "1.0.0", "repos/github.com/other/lib/1.0.0");
    home.seed_index(json!({
        "packages": [
            package("zod", "npm", "3.22.0", "repos/github.com/colinhacks/zod/3.22.0"),
            other.clone(),
        ],
        "repos": [
            repo("github.com/colinhacks/zod", "main", "repos/github.com/colinhacks/zod/main"),
        ],
    }));
    home.seed_folder("repos/github.com/colinhacks/zod/3.22.0");
    home.seed_folder("repos/github.com/colinhacks/zod/main");
    home.seed_folder("repos/github.com/other/lib/1.0.0");

    let run = home.run(&["remove", "zod", "missing-pkg", "colinhacks/zod"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(run.stdout.contains("⚠ missing-pkg not found"), "{run:?}");
    assert!(
        run.stdout.contains("Removed 2 source(s), 1 not found"),
        "{run:?}"
    );
    assert_eq!(home.index(), Some(json!({ "packages": [other] })));
    assert!(!home.exists("repos/github.com/colinhacks"));
    assert!(home.exists("repos/github.com/other/lib/1.0.0/MARKER"));
}
