use aiken_lang::ast::Tracing;
use aiken_project::{
    Project,
    telemetry::{CoverageMode, EventListener},
};
use std::path::PathBuf;

struct Silent;

impl EventListener for Silent {}

/// Type-check the standard library that ships with the compiler and run its test suite.
#[test]
fn stdlib() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("stdlib");

    let mut project = Project::new(root, Silent).expect("failed to load the standard library");

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
