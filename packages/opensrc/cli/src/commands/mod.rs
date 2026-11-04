pub mod clean;
pub mod fetch;
pub mod list;
pub mod path;
pub mod remove;
pub mod update;

use crate::core::registries::Registry;

/// The `--packages`, `--repos`, `--npm`, `--pypi` and `--crates` selection shared by `clean` and
/// `update`. No flag selects everything; a registry flag implies `--packages`.
pub struct Scope {
    pub packages: bool,
    pub repos: bool,
    pub registry: Option<Registry>,
}

impl Scope {
    pub fn from_flags(packages: bool, repos: bool, npm: bool, pypi: bool, crates: bool) -> Self {
        let registry = if npm {
            Some(Registry::Npm)
        } else if pypi {
            Some(Registry::PyPI)
        } else if crates {
            Some(Registry::Crates)
        } else {
            None
        };
        let packages = packages || registry.is_some();
        Self {
            packages: packages || !repos,
            repos: repos || !packages,
            registry,
        }
    }

    pub fn includes(&self, registry: Registry) -> bool {
        self.packages && self.registry.is_none_or(|r| r == registry)
    }
}
