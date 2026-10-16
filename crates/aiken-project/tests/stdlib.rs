use aiken_lang::ast::Tracing;
use aiken_project::{
    Project, docs,
    telemetry::{CoverageMode, EventListener},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

struct Silent;

impl EventListener for Silent {}

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Type-check the standard library that ships with the compiler and run its test suite.
#[test]
fn stdlib() {
    let mut project = Project::new(repository().join("stdlib"), Silent)
        .expect("failed to load the standard library");

    let result = project.check(
        false,
        None,
        false,
        false,
        42,
        100,
        CoverageMode::default(),
        Tracing::verbose(),
        false,
        None,
    );

    if let Err(errors) = result {
        panic!(
            "the standard library failed to check:\n{}",
            errors
                .iter()
                .map(|error| format!("{error:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// The standard library's pages on the documentation site are generated from its doc comments.
/// This fails when they're out of date; regenerate them with:
///
///     UPDATE_STDLIB_DOCS=1 cargo test -p aiken-project --test stdlib
#[test]
fn stdlib_docs() {
    let mut project = Project::new(repository().join("stdlib"), Silent)
        .expect("failed to load the standard library");

    let files = project
        .docs_files(
            false,
            docs::Format::Markdown {
                base: "/stdlib".to_string(),
            },
        )
        .unwrap_or_else(|errors| {
            panic!("failed to generate the standard library's documentation: {errors:?}")
        });

    let site = repository()
        .join("site")
        .join("src")
        .join("content")
        .join("docs")
        .join("stdlib");

    if std::env::var_os("UPDATE_STDLIB_DOCS").is_some() {
        if site.exists() {
            fs::remove_dir_all(&site).unwrap();
        }
        for file in &files {
            let path = site.join(&file.path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &file.content).unwrap();
        }
        return;
    }

    let expected: BTreeMap<PathBuf, &str> = files
        .iter()
        .map(|file| (file.path.clone(), file.content.as_str()))
        .collect();

    let actual: BTreeMap<PathBuf, String> = walk(&site)
        .into_iter()
        .map(|path| {
            let content = fs::read_to_string(&path).unwrap();
            (path.strip_prefix(&site).unwrap().to_path_buf(), content)
        })
        .collect();

    let mut stale = expected
        .iter()
        .filter(|(path, content)| actual.get(*path).map(String::as_str) != Some(**content))
        .map(|(path, _)| path.display().to_string())
        .collect::<Vec<_>>();

    stale.extend(
        actual
            .keys()
            .filter(|path| !expected.contains_key(*path))
            .map(|path| format!("{} (no longer generated)", path.display())),
    );

    assert!(
        stale.is_empty(),
        "the standard library's documentation pages are out of date:\n  {}\n\nRegenerate them with:\n\n  UPDATE_STDLIB_DOCS=1 cargo test -p aiken-project --test stdlib\n",
        stale.join("\n  ")
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return vec![];
    };

    entries
        .flat_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}
