use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use super::error::{Error, Result};
use super::registries::repo::parse_repo_spec;
use super::registries::{detect_input_type, detect_registry, Registry};

static RE_HTTPS_REPO: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"https?://([^/]+)/([^/]+)/([^/]+)").unwrap());
static RE_SSH_REPO: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"git@([^:]+):([^/]+)/(.+)").unwrap());

const OPENSRC_DIR: &str = ".opensrc";
const REPOS_DIR: &str = "repos";
const SOURCES_FILE: &str = "sources.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub registry: Registry,
    pub path: String,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub name: String,
    pub version: String,
    pub path: String,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SourcesIndex {
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packages: Option<Vec<PackageEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repos: Option<Vec<RepoEntry>>,
}

pub fn get_opensrc_dir() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("OPENSRC_HOME") {
        return Ok(PathBuf::from(home));
    }
    dirs::home_dir()
        .map(|h| h.join(OPENSRC_DIR))
        .ok_or(Error::HomeDirNotFound)
}

pub fn get_repos_dir() -> Result<PathBuf> {
    Ok(get_opensrc_dir()?.join(REPOS_DIR))
}

/// Extract host/owner/repo from a git URL.
pub fn parse_repo_url(url: &str) -> Option<(String, String, String)> {
    if let Some(caps) = RE_HTTPS_REPO.captures(url) {
        let repo = caps[3].trim_end_matches(".git").to_string();
        return Some((caps[1].to_string(), caps[2].to_string(), repo));
    }

    if let Some(caps) = RE_SSH_REPO.captures(url) {
        let repo = caps[3].trim_end_matches(".git").to_string();
        return Some((caps[1].to_string(), caps[2].to_string(), repo));
    }

    None
}

pub fn get_repo_display_name(repo_url: &str) -> Option<String> {
    parse_repo_url(repo_url).map(|(host, owner, repo)| format!("{host}/{owner}/{repo}"))
}

pub fn get_repo_path(display_name: &str, version: &str) -> Result<PathBuf> {
    Ok(get_repos_dir()?.join(display_name).join(version))
}

pub fn get_repo_relative_path(display_name: &str, version: &str) -> String {
    format!("{REPOS_DIR}/{display_name}/{version}")
}

pub fn get_absolute_path(relative_path: &str) -> Result<PathBuf> {
    Ok(get_opensrc_dir()?.join(relative_path))
}

pub fn read_sources() -> Result<SourcesIndex> {
    let path = get_opensrc_dir()?.join(SOURCES_FILE);
    if !path.exists() {
        return Ok(SourcesIndex::default());
    }
    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(index) => Ok(index),
            Err(e) => {
                let bak = path.with_extension("json.bak");
                eprintln!(
                    "Warning: {} is corrupt ({}), backing up to {}",
                    path.display(),
                    e,
                    bak.display()
                );
                let _ = fs::copy(&path, &bak);
                Ok(SourcesIndex::default())
            }
        },
        Err(_) => Ok(SourcesIndex::default()),
    }
}

pub fn list_sources() -> Result<(Vec<PackageEntry>, Vec<RepoEntry>)> {
    let index = read_sources()?;
    Ok((
        index.packages.unwrap_or_default(),
        index.repos.unwrap_or_default(),
    ))
}

/// Atomic write: serializes to a temp file then renames, so concurrent
/// readers never see a partially-written sources.json.
pub fn write_sources(packages: Vec<PackageEntry>, repos: Vec<RepoEntry>) -> Result<()> {
    let dir = get_opensrc_dir()?;
    let path = dir.join(SOURCES_FILE);

    if packages.is_empty() && repos.is_empty() {
        if path.exists() {
            fs::remove_file(&path)?;
        }
        return Ok(());
    }

    fs::create_dir_all(&dir)?;

    let index = SourcesIndex {
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
        packages: if packages.is_empty() {
            None
        } else {
            Some(packages)
        },
        repos: if repos.is_empty() { None } else { Some(repos) },
    };

    let json = serde_json::to_string_pretty(&index)?;
    let tmp = dir.join(".sources.json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn get_package_info(name: &str, registry: Registry) -> Result<Option<PackageEntry>> {
    let (packages, _) = list_sources()?;
    Ok(packages
        .into_iter()
        .find(|p| p.name == name && p.registry == registry))
}

pub fn get_repo_info(display_name: &str) -> Result<Option<RepoEntry>> {
    let (_, repos) = list_sources()?;
    Ok(repos.into_iter().find(|r| r.name == display_name))
}

pub fn extract_repo_base_path(full_path: &str) -> String {
    let parts: Vec<&str> = full_path.split('/').collect();
    if parts.len() >= 4 && parts[0] == "repos" {
        parts[..4].join("/")
    } else {
        full_path.to_string()
    }
}

pub enum CachedSource {
    Package(PackageEntry),
    Repo(RepoEntry),
}

/// Resolves a `fetch`-style spec to its sources.json entry. A package spec is looked up in its
/// detected registry first, then in the others. Fails with `InvalidRepoSpec` or `SourceNotFound`.
pub fn find_cached_source(input: &str) -> Result<CachedSource> {
    let (packages, repos) = list_sources()?;

    if detect_input_type(input) == "repo" {
        let spec =
            parse_repo_spec(input).ok_or_else(|| Error::InvalidRepoSpec(input.to_string()))?;
        let name = format!("{}/{}/{}", spec.host, spec.owner, spec.repo);
        return repos
            .into_iter()
            .find(|r| r.name == name)
            .map(CachedSource::Repo)
            .ok_or_else(|| Error::SourceNotFound(input.to_string()));
    }

    let detected = detect_registry(input);
    let name = detected.clean_spec;
    let fallbacks = [Registry::Npm, Registry::PyPI, Registry::Crates]
        .into_iter()
        .filter(|r| *r != detected.registry);
    std::iter::once(detected.registry)
        .chain(fallbacks)
        .find_map(|registry| {
            packages
                .iter()
                .find(|p| p.name == name && p.registry == registry)
                .cloned()
        })
        .map(CachedSource::Package)
        .ok_or(Error::SourceNotFound(name))
}

/// Drops `source` from the index, then deletes the folders it leaves unreferenced. Removing a
/// repo also deletes unindexed folders under its `repos/<host>/<owner>/<repo>` directory.
/// Returns whether the source's own version folder was deleted.
pub fn remove_cached_source(source: &CachedSource) -> Result<bool> {
    let (mut packages, mut repos) = list_sources()?;

    let (own_path, mut candidates) = match source {
        CachedSource::Package(pkg) => {
            packages.retain(|p| !(p.name == pkg.name && p.registry == pkg.registry));
            (pkg.path.clone(), Vec::new())
        }
        CachedSource::Repo(repo) => {
            repos.retain(|r| r.name != repo.name);
            (repo.path.clone(), repo_version_folders(&repo.name)?)
        }
    };
    candidates.push(own_path.clone());

    let deleted = write_sources_and_prune(packages, repos, &candidates)?;
    Ok(version_folder(&own_path).is_some_and(|own| deleted.contains(&own)))
}

/// Writes the index, then deletes each candidate's version folder
/// (`repos/<host>/<owner>/<repo>/<version>`) that no written entry references, then prunes the
/// empty parents. Writing first means an interruption leaves an orphan folder, never an entry
/// whose folder is gone. Returns the deleted folders, relative to the opensrc directory.
pub fn write_sources_and_prune(
    packages: Vec<PackageEntry>,
    repos: Vec<RepoEntry>,
    candidates: &[String],
) -> Result<Vec<String>> {
    let referenced: Vec<String> = packages
        .iter()
        .map(|p| p.path.as_str())
        .chain(repos.iter().map(|r| r.path.as_str()))
        .filter_map(version_folder)
        .collect();

    write_sources(packages, repos)?;

    let mut unreferenced: Vec<String> = candidates
        .iter()
        .filter_map(|c| version_folder(c))
        .filter(|folder| !referenced.iter().any(|r| r.eq_ignore_ascii_case(folder)))
        .collect();
    unreferenced.sort();
    unreferenced.dedup();

    let base = get_opensrc_dir()?;
    let mut deleted = Vec::new();
    for folder in unreferenced {
        let full = base.join(&folder);
        if full.exists() {
            fs::remove_dir_all(&full)?;
            deleted.push(folder.clone());
        }
        cleanup_empty_parent_dirs(&folder)?;
    }
    Ok(deleted)
}

fn version_folder(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').take(5).collect();
    let well_formed = parts.len() == 5
        && parts[0] == REPOS_DIR
        && parts.iter().all(|p| !matches!(*p, "" | "." | ".."));
    well_formed.then(|| parts.join("/"))
}

fn repo_version_folders(display_name: &str) -> Result<Vec<String>> {
    let dir = get_repos_dir()?.join(display_name);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut folders = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            folders.push(format!(
                "{REPOS_DIR}/{display_name}/{}",
                entry.file_name().to_string_lossy()
            ));
        }
    }
    Ok(folders)
}

fn cleanup_empty_parent_dirs(relative_path: &str) -> Result<()> {
    let parts: Vec<&str> = relative_path.split('/').collect();
    if parts.len() < 2 {
        return Ok(());
    }

    let base = get_opensrc_dir()?;
    for i in (1..parts.len()).rev() {
        let dir = base.join(parts[..i].join("/"));
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                if entries.count() == 0 {
                    let _ = fs::remove_dir(&dir);
                } else {
                    break;
                }
            }
        }
    }
    Ok(())
}

pub fn cleanup_empty_dirs(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                cleanup_empty_dirs(&entry.path());
            }
        }
    }
    if let Ok(entries) = fs::read_dir(dir) {
        if entries.count() == 0 {
            let _ = fs::remove_dir(dir);
        }
    }
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_parse_repo_url_https() {
        let result = parse_repo_url("https://github.com/colinhacks/zod").unwrap();
        assert_eq!(
            result,
            ("github.com".into(), "colinhacks".into(), "zod".into())
        );
    }

    #[test]
    fn test_parse_repo_url_ssh() {
        let result = parse_repo_url("git@github.com:colinhacks/zod.git").unwrap();
        assert_eq!(
            result,
            ("github.com".into(), "colinhacks".into(), "zod".into())
        );
    }

    #[test]
    fn test_get_repo_display_name() {
        assert_eq!(
            get_repo_display_name("https://github.com/colinhacks/zod"),
            Some("github.com/colinhacks/zod".into())
        );
    }

    #[test]
    fn test_extract_repo_base_path() {
        assert_eq!(
            extract_repo_base_path("repos/github.com/owner/repo/1.0.0/packages/sub"),
            "repos/github.com/owner/repo".to_string()
        );
        assert_eq!(extract_repo_base_path("other"), "other".to_string());
    }

    #[test]
    fn test_read_sources_corrupt_json_creates_backup() {
        let _env_guard = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("opensrc_test_corrupt");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let sources_path = tmp.join(SOURCES_FILE);
        let backup_path = tmp.join("sources.json.bak");
        fs::write(&sources_path, "NOT VALID JSON {{{").unwrap();

        std::env::set_var("OPENSRC_HOME", tmp.to_str().unwrap());
        let index = read_sources().unwrap();
        std::env::remove_var("OPENSRC_HOME");

        assert!(index.packages.is_none());
        assert!(index.repos.is_none());
        assert!(backup_path.exists(), "backup file should be created");
        let bak_content = fs::read_to_string(&backup_path).unwrap();
        assert_eq!(bak_content, "NOT VALID JSON {{{");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_sources_valid_json() {
        let _env_guard = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("opensrc_test_valid");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let sources_path = tmp.join(SOURCES_FILE);
        fs::write(
            &sources_path,
            r#"{"updatedAt":"2024-01-01T00:00:00Z","packages":[{"name":"zod","version":"3.22.0","registry":"npm","path":"repos/github.com/colinhacks/zod/v3.22.0","fetchedAt":"2024-01-01T00:00:00Z"}]}"#,
        )
        .unwrap();

        std::env::set_var("OPENSRC_HOME", tmp.to_str().unwrap());
        let index = read_sources().unwrap();
        std::env::remove_var("OPENSRC_HOME");

        assert!(index.packages.is_some());
        assert_eq!(index.packages.unwrap().len(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }
}
