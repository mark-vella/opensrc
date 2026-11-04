use crate::commands::Scope;
use crate::core::cache::{find_cached_source, list_sources, CachedSource};
use crate::core::error::{Error, Result};
use crate::core::fetcher::{update_cached_source, UpdateOutcome};

pub fn run(targets: &[String], scope: Scope, cwd: Option<&str>, quiet: bool) -> Result<()> {
    let cwd = cwd.unwrap_or(".");
    let mut sources = Vec::new();
    let mut not_found = 0u32;
    let mut had_errors = false;

    if targets.is_empty() {
        let (packages, repos) = list_sources()?;
        sources.extend(
            packages
                .into_iter()
                .filter(|p| scope.includes(p.registry))
                .map(CachedSource::Package),
        );
        if scope.repos {
            sources.extend(repos.into_iter().map(CachedSource::Repo));
        }
    }
    for target in targets {
        match find_cached_source(target) {
            Ok(source) => sources.push(source),
            Err(Error::SourceNotFound(name)) => {
                println!("  ⚠ {name} not found");
                not_found += 1;
            }
            Err(Error::InvalidRepoSpec(_)) => {
                println!("  ✗ Could not parse repo spec: {target}");
                had_errors = true;
            }
            Err(e) => return Err(e),
        }
    }

    let mut updated = 0u32;
    let mut current = 0u32;
    for source in &sources {
        let name = match source {
            CachedSource::Package(p) => &p.name,
            CachedSource::Repo(r) => &r.name,
        };
        match update_cached_source(source, cwd, !quiet) {
            Ok(UpdateOutcome::Package { from, to }) if from == to => {
                current += 1;
                if !quiet {
                    println!("  ✓ {name} {from} up to date");
                }
            }
            Ok(UpdateOutcome::Package { from, to }) => {
                updated += 1;
                if !quiet {
                    println!("  ✓ {name} {from} → {to}");
                }
            }
            Ok(UpdateOutcome::Repo { git_ref }) => {
                updated += 1;
                if !quiet {
                    println!("  ✓ {name}@{git_ref} refreshed");
                }
            }
            Err(e) => {
                had_errors = true;
                println!("  ✗ {name}: {e}");
            }
        }
    }

    if !quiet {
        let mut parts = vec![format!("Updated {updated} source(s)")];
        if current > 0 {
            parts.push(format!("{current} up to date"));
        }
        if not_found > 0 {
            parts.push(format!("{not_found} not found"));
        }
        println!("\n{}", parts.join(", "));
    }

    if had_errors {
        return Err(Error::Other(
            "Some sources could not be updated".to_string(),
        ));
    }

    Ok(())
}
