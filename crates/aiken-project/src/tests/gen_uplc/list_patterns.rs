use super::*;
use pretty_assertions::assert_eq;

fn int_list(values: &[i64]) -> Data {
    Data::list(
        values
            .iter()
            .map(|value| Data::integer((*value).into()))
            .collect(),
    )
}

fn bool_list(values: &[bool]) -> Data {
    Data::list(
        values
            .iter()
            .map(|value| Data::constr(u64::from(*value), vec![]))
            .collect(),
    )
}

fn assert_list_cases(source: &str, cases: &[(Data, i64)]) {
    let mut project = TestProject::new();
    let module = project.check(project.parse(source));
    let function = module
        .ast
        .definitions()
        .find_map(|definition| match definition {
            Definition::Fn(function) if function.name == "classify" => Some(function),
            _ => None,
        })
        .expect("source should define a classify function");

    for trace_level in [TraceLevel::Silent, TraceLevel::Compact, TraceLevel::Verbose] {
        let program: Program<DeBruijn> = project
            .new_generator(Tracing::All(trace_level))
            .generate_raw(&function.body, &function.arguments, &module.name)
            .unwrap()
            .try_into()
            .unwrap();

        for (input, expected) in cases {
            let result = program
                .apply_data(input.clone())
                .eval(ExBudget::default())
                .result();

            assert_eq!(
                result.unwrap_or_else(|error| {
                    panic!("trace level {trace_level}, input {input:?}: {error}")
                }),
                Term::integer((*expected).into()),
                "trace level {trace_level}, input {input:?}"
            );
        }
    }
}

#[test]
fn longer_literal_prefix_precedes_nonempty_fallback() {
    assert_list_cases(
        r#"
        fn classify(xs: List<Int>) -> Int {
          when xs is {
            [0, 0, ..] -> 1
            [_, ..] -> 2
            [] -> 3
          }
        }
        "#,
        &[
            (int_list(&[]), 3),
            (int_list(&[0]), 2),
            (int_list(&[1]), 2),
            (int_list(&[0, 0]), 1),
            (int_list(&[0, 0, 7]), 1),
            (int_list(&[0, 1]), 2),
            (int_list(&[1, 0]), 2),
            (int_list(&[1, 1]), 2),
            (int_list(&[0, 1, 0]), 2),
        ],
    );
}

#[test]
fn later_literals_in_longer_prefixes_precede_shorter_prefixes() {
    assert_list_cases(
        r#"
        fn classify(xs: List<Int>) -> Int {
          when xs is {
            [_, _, 0, ..] -> 1
            [_, 0, ..] -> 2
            [_, ..] -> 3
            [] -> 4
          }
        }
        "#,
        &[
            (int_list(&[]), 4),
            (int_list(&[0]), 3),
            (int_list(&[1, 0]), 2),
            (int_list(&[1, 1]), 3),
            (int_list(&[1, 2, 0]), 1),
            (int_list(&[1, 2, 0, 7]), 1),
            (int_list(&[1, 0, 0]), 1),
            (int_list(&[1, 0, 7]), 2),
            (int_list(&[1, 0, 7, 8]), 2),
            (int_list(&[1, 1, 7]), 3),
        ],
    );
}

#[test]
fn shorter_literal_prefix_keeps_clause_priority() {
    assert_list_cases(
        r#"
        fn classify(xs: List<Int>) -> Int {
          when xs is {
            [0, ..] -> 1
            [_, 1, ..] -> 2
            [_, _, 2, ..] -> 3
            [] -> 4
            _ -> 5
          }
        }
        "#,
        &[
            (int_list(&[]), 4),
            (int_list(&[0]), 1),
            (int_list(&[9]), 5),
            (int_list(&[0, 1]), 1),
            (int_list(&[0, 1, 2]), 1),
            (int_list(&[9, 1]), 2),
            (int_list(&[9, 1, 2]), 2),
            (int_list(&[9, 9, 2]), 3),
            (int_list(&[9, 9, 2, 7]), 3),
            (int_list(&[9, 9]), 5),
            (int_list(&[9, 9, 9]), 5),
        ],
    );
}

#[test]
fn exact_lists_and_named_tails_keep_clause_priority() {
    assert_list_cases(
        r#"
        fn classify(xs: List<Int>) -> Int {
          when xs is {
            [0, 0] -> 1
            [0, 0, ..rest] -> when rest is {
              [] -> 100
              [x, ..] -> x + 100
            }
            [x, y, z, w] -> x + y + z + w + 1000
            [x, ..rest] -> when rest is {
              [] -> x + 20
              [y, ..] -> x + y + 30
            }
            [] -> 0
          }
        }
        "#,
        &[
            (int_list(&[]), 0),
            (int_list(&[9]), 29),
            (int_list(&[0, 0]), 1),
            (int_list(&[0, 0, 7]), 107),
            (int_list(&[0, 0, 7, 8]), 107),
            (int_list(&[0, 0, 7, 8, 9]), 107),
            (int_list(&[1, 2, 3, 4]), 1010),
            (int_list(&[1, 2, 3]), 33),
            (int_list(&[1, 2]), 33),
            (int_list(&[1, 2, 3, 4, 5]), 33),
        ],
    );
}

#[test]
fn boolean_prefixes_precede_nonempty_fallback() {
    assert_list_cases(
        r#"
        fn classify(xs: List<Bool>) -> Int {
          when xs is {
            [True, False, ..] -> 1
            [False, ..] -> 2
            [_, ..] -> 3
            [] -> 4
          }
        }
        "#,
        &[
            (bool_list(&[]), 4),
            (bool_list(&[true]), 3),
            (bool_list(&[false]), 2),
            (bool_list(&[true, false]), 1),
            (bool_list(&[true, false, true]), 1),
            (bool_list(&[true, true]), 3),
            (bool_list(&[true, true, false]), 3),
            (bool_list(&[false, true]), 2),
            (bool_list(&[false, false]), 2),
        ],
    );
}

#[test]
fn same_length_prefixes_keep_clause_priority() {
    assert_list_cases(
        r#"
        fn classify(xs: List<Int>) -> Int {
          when xs is {
            [0, 0, ..] -> 1
            [0, _, ..] -> 2
            [1, ..] -> 3
            [_, ..] -> 4
            [] -> 5
          }
        }
        "#,
        &[
            (int_list(&[]), 5),
            (int_list(&[0]), 4),
            (int_list(&[1]), 3),
            (int_list(&[0, 0]), 1),
            (int_list(&[0, 0, 7]), 1),
            (int_list(&[0, 1]), 2),
            (int_list(&[0, 1, 7]), 2),
            (int_list(&[1, 0]), 3),
            (int_list(&[2, 0]), 4),
        ],
    );
}
