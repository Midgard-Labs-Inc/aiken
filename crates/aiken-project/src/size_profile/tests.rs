use super::{build_report, flat::measure_program};
use crate::{Project, options::BlueprintExport, telemetry::EventListener};
use aiken_lang::ast::{TraceLevel, Tracing};
use indoc::indoc;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use uplc::ast::{Constant, Data, Name, Program, Term, Type};

fn create_temp_dir() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "aiken-size-profile-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    root
}

fn make_program(term: Term<Name>) -> Program<Name> {
    Program {
        version: (1, 1, 0),
        term,
    }
}

#[test]
fn measures_all_term_types() {
    for length in [0, 1, 254, 255, 256, 510, 511] {
        let term = Term::constr(
            129,
            vec![
                Term::var("x"),
                Term::byte_string(vec![0xab; length]),
                Term::integer((-129).into()),
                Term::string("a\n\"β"),
                Term::bool(true),
                Term::unit(),
                Term::data(Data::constr(0, vec![Data::integer(1.into())])),
                Term::int_values(vec![Constant::Integer(128.into())]),
                Term::Constant(
                    Constant::ProtoPair(
                        Type::Bool,
                        Type::ByteString,
                        Constant::Bool(false).into(),
                        Constant::ByteString(vec![1, 2]).into(),
                    )
                    .into(),
                ),
                Term::Error,
                Term::add_integer().apply(Term::integer(2.into())),
            ],
        )
        .case(vec![Term::unit().delay().force(), Term::constr(0, vec![])])
        .lambda("x");
        let p = make_program(term);
        let expected_cbor = p.clone().to_debruijn().unwrap().to_cbor().unwrap();
        let measured = measure_program(p).unwrap();
        assert_eq!(measured.cbor, expected_cbor);
        let kinds: BTreeSet<_> = measured.nodes.iter().map(|n| n.kind).collect();
        assert_eq!(kinds.len(), 10);
        assert_eq!(
            measured
                .nodes
                .iter()
                .map(|n| n.exclusive_bits)
                .sum::<usize>(),
            measured.nodes[0].inclusive_bits
        );
        for node in &measured.nodes {
            assert_eq!(node.end_bit - node.start_bit, node.inclusive_bits);
            if let Some(parent) = node.parent {
                assert!(node.start_bit >= measured.nodes[parent].start_bit);
                assert!(node.end_bit <= measured.nodes[parent].end_bit);
            }
        }
    }
}

#[test]
fn handles_large_versions() {
    let measured = measure_program(Program {
        version: (128, 0, 0),
        term: Term::unit(),
    })
    .unwrap();
    assert_eq!(measured.version_bits, 32);
}

#[test]
fn matches_function_sources() {
    let measured = measure_program(make_program(
        Term::var("foo_bar_id_0")
            .lambda("foo_bar_id_0")
            .apply(Term::byte_string(vec![1; 256]).delay()),
    ))
    .unwrap();
    let functions = [json!({"uplc_name":"foo_bar", "source":{"path":"one.ak"}})];
    let data = build_report(&measured, &functions);
    let section = &data["sections"][0];
    assert_eq!(section["source"], functions[0]["source"]);
    let definition = &measured.nodes[section["definition_term_id"].as_u64().unwrap() as usize];
    assert_eq!(section["definition_bits"], definition.inclusive_bits);
    let ambiguous = [
        functions[0].clone(),
        json!({"uplc_name":"foo_bar", "source":{"path":"two.ak"}}),
    ];
    assert!(build_report(&measured, &ambiguous)["sections"][0]["source"].is_null());
}

struct Quiet;
impl EventListener for Quiet {}

#[test]
fn respects_build_options() {
    let temp = create_temp_dir();
    let root = temp.join("project");
    fs::create_dir_all(root.join("validators")).unwrap();
    let manifest = indoc! {r#"
        name = "test/profile"
        version = "0.0.0"
        plutus = "v3"
    "#};
    let source = indoc! {r#"
        validator tiny {
          else(_ctx: Data) {
            trace @"sized"
            True
          }
        }
    "#};
    fs::write(root.join("aiken.toml"), manifest).unwrap();
    fs::write(root.join("validators/tiny.ak"), source).unwrap();
    let blueprint = root.join("custom-blueprint.json");
    let report_path = root.join("build/size-profile.json");
    let mut sizes = Vec::new();
    for tracing in [Tracing::silent(), Tracing::All(TraceLevel::Verbose)] {
        let mut project = Project::new(root.clone(), Quiet).unwrap();
        project
            .build(
                false,
                tracing,
                blueprint.clone(),
                BlueprintExport::AllTypes,
                None,
            )
            .unwrap();
        let original_blueprint = fs::read(&blueprint).unwrap();
        assert_eq!(project.write_size_profile(&blueprint, tracing).unwrap(), 1);
        let first = fs::read(&report_path).unwrap();
        assert_eq!(project.write_size_profile(&blueprint, tracing).unwrap(), 1);
        assert_eq!(first, fs::read(&report_path).unwrap());
        assert_eq!(original_blueprint, fs::read(&blueprint).unwrap());
        let row: Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(row["validators"][0]["blueprint_bytes_match"], true);
        sizes.push(row["validators"][0]["flat_bytes"].as_u64().unwrap());
    }
    assert!(
        sizes[1] > sizes[0],
        "verbose trace must be included in profile"
    );
    assert_eq!(
        fs::read_to_string(root.join("aiken.toml")).unwrap(),
        manifest
    );
    assert_eq!(
        fs::read_to_string(root.join("validators/tiny.ak")).unwrap(),
        source
    );
    assert!(!root.join("plutus.json").exists());
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn handles_no_validators() {
    let root = create_temp_dir();
    fs::create_dir(root.join("lib")).unwrap();
    fs::write(
        root.join("aiken.toml"),
        "name = \"test/library\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    fs::write(root.join("lib/math.ak"), "pub fn one() { 1 }\n").unwrap();
    let mut project = Project::new(root.to_path_buf(), Quiet).unwrap();
    let blueprint = root.join("plutus.json");
    project
        .build(
            false,
            Tracing::silent(),
            blueprint.clone(),
            BlueprintExport::OnlyBinaryInterface,
            None,
        )
        .unwrap();
    assert_eq!(
        project
            .write_size_profile(&blueprint, Tracing::silent())
            .unwrap(),
        0
    );
    let report: Value =
        serde_json::from_slice(&fs::read(root.join("build/size-profile.json")).unwrap()).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert!(report["validators"].as_array().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refreshes_reports() {
    let root = create_temp_dir();
    fs::create_dir(root.join("validators")).unwrap();
    fs::write(
        root.join("aiken.toml"),
        indoc! {r#"
            name = "test/profile"
            version = "0.0.0"
        "#},
    )
    .unwrap();
    let source = include_str!("../../../../examples/hello_world/validators/hello_world.ak");
    let blueprint = root.join("plutus.json");
    let output = root.join("build/size-profile.json");
    for name in ["hello_world", "renamed"] {
        fs::write(
            root.join("validators/hello_world.ak"),
            source.replace("validator hello_world", &format!("validator {name}")),
        )
        .unwrap();
        let mut project = Project::new(root.clone(), Quiet).unwrap();
        project
            .build(
                false,
                Tracing::silent(),
                blueprint.clone(),
                BlueprintExport::OnlyBinaryInterface,
                None,
            )
            .unwrap();
        let original_blueprint = fs::read(&blueprint).unwrap();
        assert_eq!(
            project
                .write_size_profile(&blueprint, Tracing::silent())
                .unwrap(),
            2
        );
        assert_eq!(fs::read(&blueprint).unwrap(), original_blueprint);
        let contents = fs::read(&output).unwrap();
        let report: Value = serde_json::from_slice(&contents).unwrap();
        let validators = report["validators"].as_array().unwrap();
        assert_eq!(validators.len(), 1);
        let script = &validators[0];
        assert_eq!(script["title"], format!("hello_world.{name}"));
        assert_eq!(
            script["handlers"],
            json!([
                format!("hello_world.{name}.spend"),
                format!("hello_world.{name}.else"),
            ])
        );
        assert_eq!(script["blueprint_bytes_match"], true);
        let section = script["sections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|section| {
                section["source"]["path"]
                    == "build/packages/aiken-stdlib/lib/aiken/collection/list.ak"
            })
            .unwrap();
        let location = &section["source"];
        let function_source =
            fs::read_to_string(root.join(location["path"].as_str().unwrap())).unwrap();
        let line = location["start_line"].as_u64().unwrap() as usize;
        assert!(
            function_source
                .lines()
                .nth(line - 1)
                .unwrap()
                .starts_with("pub fn ")
        );
        if name == "renamed" {
            for code in [
                "00".to_string(),
                format!(
                    "{}00",
                    serde_json::from_slice::<Value>(&original_blueprint).unwrap()["validators"][1]
                        ["compiledCode"]
                        .as_str()
                        .unwrap()
                ),
            ] {
                let mut corrupted: Value = serde_json::from_slice(&original_blueprint).unwrap();
                corrupted["validators"][1]["compiledCode"] = json!(code);
                fs::write(&blueprint, serde_json::to_vec(&corrupted).unwrap()).unwrap();
                assert!(
                    project
                        .write_size_profile(&blueprint, Tracing::silent())
                        .is_err()
                );
                assert_eq!(fs::read(&output).unwrap(), contents);
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn profiles_distinct_module_paths() {
    let root = create_temp_dir();
    fs::write(
        root.join("aiken.toml"),
        indoc! {r#"
            name = "test/profile"
            version = "0.0.0"
        "#},
    )
    .unwrap();
    for module in ["foo_bar/one", "foo/bar_one"] {
        let path = root.join(format!("validators/{module}.ak"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "validator v { else(_ctx: Data) { True } }").unwrap();
    }
    let mut project = Project::new(root.clone(), Quiet).unwrap();
    let blueprint = root.join("plutus.json");
    project
        .build(
            false,
            Tracing::silent(),
            blueprint.clone(),
            BlueprintExport::OnlyBinaryInterface,
            None,
        )
        .unwrap();
    assert_eq!(
        project
            .write_size_profile(&blueprint, Tracing::silent())
            .unwrap(),
        2
    );
    let report: Value =
        serde_json::from_slice(&fs::read(root.join("build/size-profile.json")).unwrap()).unwrap();
    let titles: BTreeSet<_> = report["validators"]
        .as_array()
        .unwrap()
        .iter()
        .map(|script| script["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, BTreeSet::from(["foo_bar/one.v", "foo/bar_one.v"]));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn preserves_blueprint_output() {
    let root = create_temp_dir();
    fs::create_dir(root.join("validators")).unwrap();
    fs::write(
        root.join("aiken.toml"),
        indoc! {r#"
            name = "test/profile"
            version = "0.0.0"
        "#},
    )
    .unwrap();
    fs::write(
        root.join("validators/tiny.ak"),
        "validator tiny { else(_ctx: Data) { True } }",
    )
    .unwrap();
    for path in ["build/size-profile/plutus.json", "build/size-profile.json"] {
        let blueprint = root.join(path);
        fs::create_dir_all(blueprint.parent().unwrap()).unwrap();
        let mut project = Project::new(root.clone(), Quiet).unwrap();
        project
            .build(
                false,
                Tracing::silent(),
                blueprint.clone(),
                BlueprintExport::OnlyBinaryInterface,
                None,
            )
            .unwrap();
        let contents = fs::read(&blueprint).unwrap();
        let result = project.write_size_profile(&blueprint, Tracing::silent());
        if path == "build/size-profile.json" {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap(), 1);
        }
        assert_eq!(fs::read(&blueprint).unwrap(), contents);
    }
    fs::remove_dir_all(root).unwrap();
}
