use crate::core::cache::{find_cached_source, remove_cached_source, CachedSource};
use crate::core::error::{Error, Result};

pub fn run(items: &[String]) -> Result<()> {
    let mut removed = 0u32;
    let mut not_found = 0u32;
    let mut had_errors = false;

    for item in items {
        let source = match find_cached_source(item) {
            Ok(source) => source,
            Err(Error::SourceNotFound(name)) => {
                println!("  ⚠ {name} not found");
                not_found += 1;
                continue;
            }
            Err(Error::InvalidRepoSpec(_)) => {
                println!("  ✗ Could not parse repo spec: {item}");
                had_errors = true;
                continue;
            }
            Err(e) => return Err(e),
        };

        match remove_cached_source(&source) {
            Ok(folder_deleted) => {
                match &source {
                    CachedSource::Package(pkg) => {
                        println!("  ✓ Removed {} ({})", pkg.name, pkg.registry);
                        if folder_deleted {
                            println!("    → Also removed repo (no other packages use it)");
                        }
                    }
                    CachedSource::Repo(repo) => println!("  ✓ Removed {}", repo.name),
                }
                removed += 1;
            }
            Err(e) => {
                println!("  ✗ Error removing {item}: {e}");
                had_errors = true;
            }
        }
    }

    let nf_msg = if not_found > 0 {
        format!(", {not_found} not found")
    } else {
        String::new()
    };
    println!("\nRemoved {removed} source(s){nf_msg}");

    if had_errors {
        return Err(Error::Other("Some items could not be removed".to_string()));
    }

    Ok(())
}
