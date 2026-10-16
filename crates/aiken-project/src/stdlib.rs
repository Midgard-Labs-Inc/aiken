//! The standard library ships with the compiler rather than being fetched as a dependency.
//! Its sources live in `stdlib/` at the root of the repository and are embedded at build
//! time (see `build.rs`); projects get them written into their build directory.

use crate::{package_name::PackageName, paths};
use std::{collections::HashSet, fs, io, path::Path};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/stdlib.rs"));
}

pub fn package_name() -> PackageName {
    PackageName {
        owner: "aiken".to_string(),
        repo: "stdlib".to_string(),
    }
}

/// Packages that are part of the bundled standard library, and thus can no longer be
/// declared as dependencies.
pub fn is_bundled(name: &PackageName) -> bool {
    matches!(
        (name.owner.as_str(), name.repo.as_str()),
        ("aiken", "stdlib") | ("aiken-lang", "stdlib") | ("aiken-lang", "fuzz")
    )
}

/// Module namespaces reserved for the standard library: a project cannot define modules
/// under them.
pub fn is_reserved_module(name: &str) -> bool {
    ["aiken", "cardano"]
        .iter()
        .any(|namespace| name == *namespace || name.starts_with(&format!("{namespace}/")))
}

/// Write the standard library into the project's build directory, rewriting only the
/// modules that differ and removing any that no longer exist.
pub fn materialize(root: &Path) -> io::Result<()> {
    let lib = root
        .join(paths::build_deps_package(&package_name()))
        .join("lib");

    for (module, code) in embedded::MODULES {
        let path = lib.join(module);

        if fs::read_to_string(&path).ok().as_deref() != Some(*code) {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, code)?;
        }
    }

    let known: HashSet<_> = embedded::MODULES
        .iter()
        .map(|(module, _)| lib.join(module))
        .collect();

    for entry in walkdir::WalkDir::new(&lib)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        if !known.contains(entry.path()) {
            fs::remove_file(entry.path())?;
        }
    }

    Ok(())
}
