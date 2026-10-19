use super::TestProject;
use aiken_lang::ast::{Definition, TraceLevel, Tracing};
use pallas_primitives::conway::Language;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uplc::{
    ast::{Data, DeBruijn, NamedDeBruijn, Program, Term},
    machine::{
        Error as EvaluationError, Machine,
        cost_model::{CostModel, ExBudget},
    },
};

const PROTOCOL_MAJOR_VERSION: u16 = 11;
const CEK_SLIPPAGE: u32 = 1;

struct Input {
    name: &'static str,
    arguments: Vec<Data>,
    expected: Option<i64>,
}

struct Case {
    name: &'static str,
    source: String,
    inputs: Vec<Input>,
}

fn integer(value: i64) -> Data {
    Data::integer(value.into())
}

fn list(values: &[i64]) -> Data {
    Data::list(values.iter().copied().map(integer).collect())
}

fn boolean(value: bool) -> Data {
    Data::constr(u64::from(value), vec![])
}

fn constant_case(name: &'static str, payload: &str, module_constant: bool) -> Case {
    let binding = if module_constant { "const" } else { "let" };
    let declaration = format!("{binding} payload = #\"{payload}\"");
    let (outside, inside) = if module_constant {
        (declaration.as_str(), "")
    } else {
        ("", declaration.as_str())
    };

    Case {
        name,
        source: format!(
            r#"
            use aiken/builtin
            {outside}
            pub fn observe(index: Int) -> Int {{
              {inside}
              builtin.index_bytearray(payload, index)
                + builtin.index_bytearray(payload, index + 1)
            }}
            "#
        ),
        inputs: vec![
            Input {
                name: "first_pair",
                arguments: vec![integer(0)],
                expected: Some(1),
            },
            Input {
                name: "second_pair",
                arguments: vec![integer(1)],
                expected: Some(3),
            },
        ],
    }
}

fn cases() -> Vec<Case> {
    let payload = hex::encode((0..=255).chain(0..=255).collect::<Vec<u8>>());

    vec![
        constant_case("large_local_constant", &payload, false),
        constant_case("large_module_constant", &payload, true),
        constant_case("small_local_constant", "00010203", false),
        Case {
            name: "late_record_field",
            source: r#"
                pub type Record {
                  first: Int,
                  second: Int,
                  third: Int,
                  fourth: Int,
                  fifth: Int,
                  sixth: Int,
                }

                pub fn observe(record: Record) -> Int {
                  record.sixth
                }
                "#
            .into(),
            inputs: vec![
                Input {
                    name: "positive",
                    arguments: vec![Data::constr(
                        0,
                        [1, 2, 3, 4, 5, 6].into_iter().map(integer).collect(),
                    )],
                    expected: Some(6),
                },
                Input {
                    name: "negative",
                    arguments: vec![Data::constr(
                        0,
                        [0, 0, 0, 0, 0, -9].into_iter().map(integer).collect(),
                    )],
                    expected: Some(-9),
                },
            ],
        },
        Case {
            name: "list_prefix",
            source: r#"
                pub fn observe(xs: List<Int>) -> Int {
                  expect [first, second, third, ..] = xs
                  first + second + third
                }
                "#
            .into(),
            inputs: vec![
                Input {
                    name: "exact_prefix",
                    arguments: vec![list(&[1, 2, 3])],
                    expected: Some(6),
                },
                Input {
                    name: "longer_list",
                    arguments: vec![list(&[1, 2, 3, 4, 5])],
                    expected: Some(6),
                },
                Input {
                    name: "short_list",
                    arguments: vec![list(&[1, 2])],
                    expected: None,
                },
                Input {
                    name: "empty_list",
                    arguments: vec![list(&[])],
                    expected: None,
                },
            ],
        },
        Case {
            name: "expect_before_branch",
            source: r#"
                pub fn observe(raw: Data, use_value: Bool) -> Int {
                  expect xs: List<Int> = raw
                  if use_value {
                    when xs is {
                      [] -> 0
                      [head, ..] -> head
                    }
                  } else {
                    7
                  }
                }
                "#
            .into(),
            inputs: vec![
                Input {
                    name: "valid_used",
                    arguments: vec![list(&[42]), boolean(true)],
                    expected: Some(42),
                },
                Input {
                    name: "valid_unused",
                    arguments: vec![list(&[42]), boolean(false)],
                    expected: Some(7),
                },
                Input {
                    name: "wrong_shape_unused",
                    arguments: vec![integer(42), boolean(false)],
                    expected: None,
                },
                Input {
                    name: "wrong_element_unused",
                    arguments: vec![Data::list(vec![Data::bytestring(vec![42])]), boolean(false)],
                    expected: None,
                },
            ],
        },
    ]
}

fn compile(source: &str, tracing: Tracing) -> Program<DeBruijn> {
    let mut project = TestProject::new();
    let module = project.check(project.parse(source));
    let function = module
        .ast
        .definitions()
        .find_map(|definition| match definition {
            Definition::Fn(function) if function.name == "observe" => Some(function),
            _ => None,
        })
        .expect("each cost case must define observe");

    project
        .new_generator(tracing)
        .generate_raw(&function.body, &function.arguments, &module.name)
        .expect("cost case must compile")
        .try_into()
        .expect("compiled program must be closed")
}

fn measure(case: &Case, tracing: Tracing) -> Value {
    let program = compile(&case.source, tracing);
    // Measure the function before applying inputs. Input bytes are not script bytes.
    let flat_bytes = program.to_flat().expect("program must encode").len();
    let cbor_bytes = program.to_cbor().expect("program must encode").len();
    let mut inputs = BTreeMap::new();

    for input in &case.inputs {
        let applied: Program<NamedDeBruijn> = input
            .arguments
            .iter()
            .fold(program.clone(), |program, argument| {
                program.apply_data(argument.clone())
            })
            .into();
        let initial_budget = ExBudget::default();
        // Charge each CEK step so failures cannot leave pending steps uncounted.
        let mut machine = Machine::new_with_protocol(
            Language::PlutusV3,
            PROTOCOL_MAJOR_VERSION,
            CostModel::default_for_language_and_protocol(
                &Language::PlutusV3,
                PROTOCOL_MAJOR_VERSION,
            ),
            initial_budget,
            CEK_SLIPPAGE,
        );
        let result = machine.run(applied.term);
        let budget = initial_budget - machine.ex_budget;

        match input.expected {
            Some(expected) => assert_eq!(
                result.expect("cost case must return a value"),
                Term::integer(expected.into()),
                "{} / {} / {tracing:?}",
                case.name,
                input.name,
            ),
            None => {
                let error = match result {
                    Ok(value) => panic!(
                        "{} / {} / {tracing:?} must fail, got {value:?}",
                        case.name, input.name,
                    ),
                    Err(error) => error,
                };
                assert!(
                    !matches!(
                        error,
                        EvaluationError::OutOfExError(_)
                            | EvaluationError::InvalidStepKind(_)
                            | EvaluationError::OpenTermEvaluated(_)
                            | EvaluationError::MachineNeverReachedDone
                            | EvaluationError::BuiltinNotAvailable(_)
                            | EvaluationError::ValueConstantNotAvailable
                    ),
                    "{} / {} / {tracing:?} has a budget or machine setup error: {error}",
                    case.name,
                    input.name,
                );
            }
        }

        assert!(
            inputs
                .insert(
                    input.name,
                    json!({
                        "result": input.expected,
                        "cpu_units": budget.cpu,
                        "memory_units": budget.mem,
                    }),
                )
                .is_none(),
            "input names must be unique",
        );
    }

    json!({
        "uplc_version": program.version,
        "flat_bytes": flat_bytes,
        "cbor_bytes": cbor_bytes,
        "inputs": inputs,
    })
}

#[test]
fn codegen_cost_baseline() {
    let mut results = BTreeMap::new();
    for case in cases() {
        let mut modes = BTreeMap::new();
        for (name, level) in [
            ("silent", TraceLevel::Silent),
            ("compact", TraceLevel::Compact),
            ("verbose", TraceLevel::Verbose),
        ] {
            modes.insert(name, measure(&case, Tracing::All(level)));
        }
        assert!(results.insert(case.name, modes).is_none());
    }

    insta::assert_json_snapshot!(json!({
        "schema_version": 1,
        "plutus_version": "v3",
        "protocol_major_version": PROTOCOL_MAJOR_VERSION,
        "cost_model": "default_for_language_and_protocol",
        "cek_slippage": CEK_SLIPPAGE,
        "cases": results,
    }));
}
