#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

pub struct TestHome {
    root: tempfile::TempDir,
}

impl TestHome {
    pub fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("create temp dir"),
        }
    }

    pub fn opensrc_home(&self) -> PathBuf {
        self.root.path().join("opensrc")
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.opensrc_home().join(relative)
    }

    pub fn exists(&self, relative: &str) -> bool {
        self.path(relative).exists()
    }

    pub fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_opensrc"));
        cmd.env("OPENSRC_HOME", self.opensrc_home())
            .env("HOME", self.root.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GITLAB_TOKEN")
            .env_remove("BITBUCKET_TOKEN")
            .current_dir(self.root.path());
        cmd
    }

    pub fn run(&self, args: &[&str]) -> Run {
        let output = self.command().args(args).output().expect("spawn opensrc");
        Run::from(output)
    }

    pub fn seed_index(&self, index: Value) {
        let home = self.opensrc_home();
        fs::create_dir_all(&home).expect("create OPENSRC_HOME");
        fs::write(
            home.join("sources.json"),
            serde_json::to_string_pretty(&index).expect("serialize index"),
        )
        .expect("write sources.json");
    }

    pub fn seed_folder(&self, relative: &str) {
        let dir = self.path(relative);
        fs::create_dir_all(&dir).expect("create source folder");
        fs::write(dir.join("MARKER"), relative).expect("write marker");
    }

    /// sources.json without the volatile `updatedAt`, or `None` when the file is absent.
    pub fn index(&self) -> Option<Value> {
        let path = self.opensrc_home().join("sources.json");
        if !path.exists() {
            return None;
        }
        let mut index: Value =
            serde_json::from_str(&fs::read_to_string(path).expect("read sources.json"))
                .expect("parse sources.json");
        index
            .as_object_mut()
            .expect("index is an object")
            .remove("updatedAt");
        Some(index)
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }
}

pub struct Run {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl From<Output> for Run {
    fn from(output: Output) -> Self {
        Self {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exit {:?}\n--- stdout\n{}--- stderr\n{}",
            self.code, self.stdout, self.stderr
        )
    }
}
