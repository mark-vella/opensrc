mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{Run, TestHome};
use serde_json::{json, Value};

const FETCHED_AT: &str = "2026-01-01T00:00:00Z";

fn repo(name: &str, version: &str) -> Value {
    json!({
        "name": name,
        "version": version,
        "path": format!("repos/{name}/{version}"),
        "fetchedAt": FETCHED_AT,
    })
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Fixture {
    work: PathBuf,
    bare: PathBuf,
}

impl Fixture {
    fn new(home: &TestHome, owner: &str, repo: &str) -> Self {
        let work = home.root().join("work").join(owner).join(repo);
        let bare = home
            .root()
            .join("fixtures/github.com")
            .join(owner)
            .join(repo);
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(&bare).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        git(&bare, &["init", "-q", "--bare"]);
        Self { work, bare }
    }

    fn commit(&self, files: &[(&str, Option<&str>)]) -> &Self {
        for (name, content) in files {
            let path = self.work.join(name);
            match content {
                Some(c) => fs::write(path, c).unwrap(),
                None => fs::remove_file(path).unwrap(),
            }
        }
        git(&self.work, &["add", "-A"]);
        git(&self.work, &["commit", "-q", "-m", "change"]);
        self
    }

    fn tag(&self, name: &str) -> &Self {
        git(&self.work, &["tag", name]);
        self
    }

    fn publish(&self) {
        git(
            &self.work,
            &[
                "push",
                "-q",
                "-f",
                self.bare.to_str().unwrap(),
                "refs/heads/*:refs/heads/*",
                "refs/tags/*:refs/tags/*",
            ],
        );
    }
}

fn update(home: &TestHome, args: &[&str]) -> Run {
    let rewrite = format!(
        "url.file://{}/.insteadOf",
        home.root().join("fixtures/github.com").display()
    );
    let output = home
        .command()
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", rewrite)
        .env("GIT_CONFIG_VALUE_0", "https://github.com/")
        .arg("update")
        .args(args)
        .output()
        .expect("spawn opensrc");
    Run::from(output)
}

fn seed_files(home: &TestHome, relative: &str, files: &[(&str, &str)]) {
    let dir = home.path(relative);
    fs::create_dir_all(&dir).unwrap();
    for (name, content) in files {
        fs::write(dir.join(name), content).unwrap();
    }
}

fn read(home: &TestHome, relative: &str) -> String {
    fs::read_to_string(home.path(relative)).unwrap()
}

fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let key = path.strip_prefix(dir).unwrap().display().to_string();
                files.insert(key, fs::read(&path).unwrap());
            }
        }
    }
    files
}

fn take_repo_fetched_at(index: &mut Value, i: usize) -> String {
    index["repos"][i]
        .as_object_mut()
        .unwrap()
        .remove("fetchedAt")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

fn staging_is_empty(home: &TestHome) -> bool {
    let staging = home.path(".staging");
    !staging.exists() || fs::read_dir(staging).unwrap().next().is_none()
}

fn widget_on_main(home: &TestHome) -> Fixture {
    let fixture = Fixture::new(home, "acme", "widget");
    fixture.commit(&[("f", Some("v1")), ("stale.txt", Some("old"))]);
    fixture.commit(&[("f", Some("v2")), ("stale.txt", None)]);
    fixture.publish();
    fixture
}

#[test]
fn refreshes_branch_repo_in_place() {
    let home = TestHome::new();
    widget_on_main(&home);
    home.seed_index(json!({ "repos": [repo("github.com/acme/widget", "main")] }));
    seed_files(
        &home,
        "repos/github.com/acme/widget/main",
        &[("f", "v1"), ("stale.txt", "old")],
    );

    let run = update(&home, &[]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert_eq!(read(&home, "repos/github.com/acme/widget/main/f"), "v2");
    assert!(!home.exists("repos/github.com/acme/widget/main/stale.txt"));
    assert!(!home.exists("repos/github.com/acme/widget/main/.git"));
    let mut index = home.index().unwrap();
    assert_ne!(take_repo_fetched_at(&mut index, 0), FETCHED_AT);
    assert_eq!(
        index,
        json!({ "repos": [{
            "name": "github.com/acme/widget",
            "version": "main",
            "path": "repos/github.com/acme/widget/main",
        }] })
    );
}

#[test]
fn tag_pinned_repo_keeps_tag_content_after_main_moves() {
    let home = TestHome::new();
    let fixture = Fixture::new(&home, "acme", "widget");
    fixture.commit(&[("f", Some("v1"))]).tag("v1.0.0");
    fixture.commit(&[("f", Some("v2"))]);
    fixture.publish();
    home.seed_index(json!({ "repos": [repo("github.com/acme/widget", "v1.0.0")] }));
    seed_files(
        &home,
        "repos/github.com/acme/widget/v1.0.0",
        &[("f", "corrupt"), ("stale.txt", "old")],
    );

    let run = update(&home, &[]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert_eq!(read(&home, "repos/github.com/acme/widget/v1.0.0/f"), "v1");
    assert!(!home.exists("repos/github.com/acme/widget/v1.0.0/stale.txt"));
}

#[test]
fn missing_ref_keeps_old_copy_and_other_repos_still_refresh() {
    let home = TestHome::new();
    widget_on_main(&home);
    let legacy = Fixture::new(&home, "acme", "legacy");
    legacy.commit(&[("f", Some("main-content"))]);
    legacy.publish();
    let gone = repo("github.com/acme/legacy", "gone");
    home.seed_index(json!({ "repos": [gone.clone(), repo("github.com/acme/widget", "main")] }));
    seed_files(
        &home,
        "repos/github.com/acme/legacy/gone",
        &[("f", "pinned"), ("extra.txt", "keep")],
    );
    seed_files(
        &home,
        "repos/github.com/acme/widget/main",
        &[("f", "v1"), ("stale.txt", "old")],
    );
    let before = snapshot(&home.path("repos/github.com/acme/legacy/gone"));

    let run = update(&home, &[]);

    assert_eq!(run.code, Some(1), "{run:?}");
    assert!(run.stdout.contains("✗ github.com/acme/legacy:"), "{run:?}");
    assert!(
        run.stderr.contains("Some sources could not be updated"),
        "{run:?}"
    );
    assert_eq!(
        snapshot(&home.path("repos/github.com/acme/legacy/gone")),
        before
    );
    let index = home.index().unwrap();
    assert_eq!(index["repos"][0], gone);
    assert_eq!(read(&home, "repos/github.com/acme/widget/main/f"), "v2");
    assert!(!home.exists("repos/github.com/acme/widget/main/stale.txt"));
    assert!(staging_is_empty(&home));
}

#[test]
fn named_target_refreshes_only_that_repo() {
    let home = TestHome::new();
    widget_on_main(&home);
    let gadget = Fixture::new(&home, "acme", "gadget");
    gadget.commit(&[("f", Some("v2"))]);
    gadget.publish();
    let gadget_entry = repo("github.com/acme/gadget", "main");
    home.seed_index(json!({
        "repos": [repo("github.com/acme/widget", "main"), gadget_entry.clone()]
    }));
    seed_files(&home, "repos/github.com/acme/widget/main", &[("f", "v1")]);
    seed_files(
        &home,
        "repos/github.com/acme/gadget/main",
        &[("f", "v1"), ("stale.txt", "old")],
    );

    let run = update(&home, &["acme/widget"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert_eq!(read(&home, "repos/github.com/acme/widget/main/f"), "v2");
    assert_eq!(read(&home, "repos/github.com/acme/gadget/main/f"), "v1");
    assert!(home.exists("repos/github.com/acme/gadget/main/stale.txt"));
    assert_eq!(home.index().unwrap()["repos"][1], gadget_entry);
}

#[test]
fn packages_flag_skips_repos() {
    let home = TestHome::new();
    widget_on_main(&home);
    let seeded = json!({ "repos": [repo("github.com/acme/widget", "main")] });
    home.seed_index(seeded.clone());
    seed_files(
        &home,
        "repos/github.com/acme/widget/main",
        &[("f", "v1"), ("stale.txt", "old")],
    );

    let run = update(&home, &["--packages"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert_eq!(read(&home, "repos/github.com/acme/widget/main/f"), "v1");
    assert!(home.exists("repos/github.com/acme/widget/main/stale.txt"));
    assert_eq!(home.index(), Some(seeded));
}

#[test]
fn second_update_converges_and_leaves_no_staging() {
    let home = TestHome::new();
    widget_on_main(&home);
    home.seed_index(json!({ "repos": [repo("github.com/acme/widget", "main")] }));
    seed_files(
        &home,
        "repos/github.com/acme/widget/main",
        &[("f", "v1"), ("stale.txt", "old")],
    );

    let first = update(&home, &[]);
    assert_eq!(first.code, Some(0), "{first:?}");
    let disk_once = snapshot(&home.path("repos"));
    let mut index_once = home.index().unwrap();
    take_repo_fetched_at(&mut index_once, 0);

    let second = update(&home, &[]);

    assert_eq!(second.code, Some(0), "{second:?}");
    assert_eq!(snapshot(&home.path("repos")), disk_once);
    let mut index_twice = home.index().unwrap();
    take_repo_fetched_at(&mut index_twice, 0);
    assert_eq!(index_twice, index_once);
    assert_eq!(read(&home, "repos/github.com/acme/widget/main/f"), "v2");
    assert!(staging_is_empty(&home));
}

#[test]
fn package_matching_lockfile_in_cwd_is_up_to_date() {
    let home = TestHome::new();
    let zod = json!({
        "name": "zod",
        "version": "3.22.0",
        "registry": "npm",
        "path": "repos/github.com/colinhacks/zod/3.22.0",
        "fetchedAt": FETCHED_AT,
    });
    home.seed_index(json!({ "packages": [zod.clone()] }));
    home.seed_folder("repos/github.com/colinhacks/zod/3.22.0");
    let project = home.root().join("project");
    fs::create_dir_all(project.join("node_modules/zod")).unwrap();
    fs::write(
        project.join("node_modules/zod/package.json"),
        r#"{"name":"zod","version":"3.22.0"}"#,
    )
    .unwrap();

    let run = update(&home, &["zod", "--cwd", project.to_str().unwrap()]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(run.stdout.contains("zod 3.22.0 up to date"), "{run:?}");
    assert_eq!(home.index(), Some(json!({ "packages": [zod] })));
    assert!(home.exists("repos/github.com/colinhacks/zod/3.22.0/MARKER"));
}

#[test]
fn missing_target_is_reported_and_exits_zero() {
    let home = TestHome::new();
    widget_on_main(&home);
    home.seed_index(json!({ "repos": [repo("github.com/acme/widget", "main")] }));
    seed_files(&home, "repos/github.com/acme/widget/main", &[("f", "v1")]);

    let run = update(&home, &["acme/widget", "nope"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(run.stdout.contains("⚠ nope not found"), "{run:?}");
    assert!(run.stdout.contains("1 not found"), "{run:?}");
    assert_eq!(read(&home, "repos/github.com/acme/widget/main/f"), "v2");
}

#[test]
#[ignore = "network: npm registry and github.com"]
fn package_update_moves_to_latest_and_prunes_old_version() {
    let home = TestHome::new();

    let fetch = home.run(&["fetch", "zod@3.22.0"]);
    assert_eq!(fetch.code, Some(0), "{fetch:?}");
    assert!(home.exists("repos/github.com/colinhacks/zod/3.22.0"));

    let run = home.run(&["update", "zod"]);

    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(run.stdout.contains("zod 3.22.0 → "), "{run:?}");
    let index = home.index().unwrap();
    let entry = &index["packages"][0];
    assert_eq!(entry["name"], "zod");
    assert_ne!(entry["version"], "3.22.0");
    assert!(!home.exists("repos/github.com/colinhacks/zod/3.22.0"));
    assert!(home.exists(entry["path"].as_str().unwrap()));
}
