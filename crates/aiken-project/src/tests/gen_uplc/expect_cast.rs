//! `expect` casts are the runtime check of a program: casting a `Data` into an `Int`, a
//! `ByteArray`, a `List` or a `Pairs` is an `unIData` / `unBData` / `unListData` / `unMapData`,
//! and the shrinker cancels `iData (unIData x)` into `x`. That is only right for trusted data:
//! when the cast comes from an `expect`, cancelling it accepts any `x`.
use super::*;
use aiken_lang::ast::TypedFunction;
use pretty_assertions::assert_eq;
use uplc::{ast::NamedDeBruijn, optimize::shrinker::CHECKED_CAST};

const TRACINGS: [Tracing; 2] = [
    Tracing::All(TraceLevel::Silent),
    Tracing::All(TraceLevel::Verbose),
];

fn observe(project: &mut TestProject, source: &str) -> (TypedFunction, String) {
    let module = project.check(project.parse(source));

    let function = module
        .ast
        .definitions()
        .find_map(|definition| match definition {
            Definition::Fn(function) if function.name == "observe" => Some(function.clone()),
            _ => None,
        })
        .expect("source should define an observe function");

    (function, module.name.clone())
}

/// Compile `observe`, without running the shrinker.
fn compile_unoptimized(source: &str, tracing: Tracing) -> Program<Name> {
    let mut project = TestProject::new();

    let (function, module) = observe(&mut project, source);

    project
        .new_generator(tracing)
        .generate_raw_unoptimized(&function.body, &function.arguments, &module)
        .unwrap()
}

fn compile(source: &str, tracing: Tracing) -> Program<Name> {
    let mut project = TestProject::new();

    let (function, module) = observe(&mut project, source);

    project
        .new_generator(tracing)
        .generate_raw(&function.body, &function.arguments, &module)
        .unwrap()
}

fn evaluate(program: &Program<Name>, input: &Data) -> Result<Term<NamedDeBruijn>, String> {
    let program: Program<DeBruijn> = program.clone().try_into().unwrap();

    program
        .apply_data(input.clone())
        .eval(ExBudget::default())
        .result()
        .map_err(|error| error.to_string())
}

fn count_builtin<T>(term: &Term<T>, function: DefaultFunction) -> usize {
    match term {
        Term::Builtin(builtin) => usize::from(*builtin == function),
        Term::Var(_) | Term::Constant(_) | Term::Error => 0,
        Term::Delay(body) | Term::Force(body) | Term::Lambda { body, .. } => {
            count_builtin(body, function)
        }
        Term::Apply {
            function: applied,
            argument,
        } => count_builtin(applied, function) + count_builtin(argument, function),
        Term::Constr { fields, .. } => fields
            .iter()
            .map(|field| count_builtin(field, function))
            .sum(),
        Term::Case { constr, branches } => {
            count_builtin(constr, function)
                + branches
                    .iter()
                    .map(|branch| count_builtin(branch, function))
                    .sum::<usize>()
        }
    }
}

fn count_checked_casts(term: &Term<Name>) -> usize {
    match term {
        Term::Var(name) => usize::from(name.text == CHECKED_CAST),
        Term::Constant(_) | Term::Error | Term::Builtin(_) => 0,
        Term::Delay(body) | Term::Force(body) | Term::Lambda { body, .. } => {
            count_checked_casts(body)
        }
        Term::Apply { function, argument } => {
            count_checked_casts(function) + count_checked_casts(argument)
        }
        Term::Constr { fields, .. } => fields.iter().map(count_checked_casts).sum(),
        Term::Case { constr, branches } => {
            count_checked_casts(constr) + branches.iter().map(count_checked_casts).sum::<usize>()
        }
    }
}

struct Scenario {
    name: &'static str,
    source: &'static str,
    /// The check that an `expect` performs, and that must survive the upcast.
    check: DefaultFunction,
    /// Input the expect accepts, and what the function returns for it.
    good: Data,
    returned: Data,
    /// Input the expect must reject.
    bad: Data,
}

fn int(n: i64) -> Data {
    Data::integer(n.into())
}

fn bytes(b: &[u8]) -> Data {
    Data::bytestring(b.to_vec())
}

fn pair(fst: Data, snd: Data) -> Data {
    Data::list(vec![fst, snd])
}

fn pairs() -> Data {
    Data::map(vec![(int(1), int(2))])
}

fn list() -> Data {
    Data::list(vec![int(1)])
}

fn assert_check_kept(scenarios: &[Scenario]) {
    for scenario in scenarios {
        for tracing in TRACINGS {
            let program = compile(scenario.source, tracing);

            let context = format!("{} ({tracing:?})", scenario.name);

            // The whole emitted program, so that any change to how the cast is compiled shows.
            let level = if tracing == Tracing::All(TraceLevel::Silent) {
                "silent"
            } else {
                "verbose"
            };
            let snapshot = format!(
                "{}_{level}",
                scenario
                    .name
                    .to_lowercase()
                    .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
            );
            insta::assert_snapshot!(snapshot, program.to_pretty());

            assert_eq!(
                count_checked_casts(&program.term),
                0,
                "{context}: the marker must not survive optimization\n{}",
                program.to_pretty(),
            );

            // With traces, the check is a `chooseData` that is told what to do otherwise,
            // rather than the `unXData` itself.
            if tracing == Tracing::All(TraceLevel::Silent) {
                assert!(
                    count_builtin(&program.term, scenario.check) > 0,
                    "{context}: the {} check was optimized away\n{}",
                    scenario.check,
                    program.to_pretty(),
                );
            }

            assert_eq!(
                evaluate(&program, &scenario.good),
                Ok(Term::data(scenario.returned.clone())),
                "{context}: the accepted input must still be accepted\n{}",
                program.to_pretty(),
            );

            assert!(
                evaluate(&program, &scenario.bad).is_err(),
                "{context}: the rejected input is accepted\n{}",
                program.to_pretty(),
            );
        }
    }
}

fn plain_expects() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "Int",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect n: Int = d
                  as_data(n)
                }
            "#,
            check: DefaultFunction::UnIData,
            good: int(5),
            returned: int(5),
            bad: bytes(&[0]),
        },
        Scenario {
            name: "ByteArray",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect b: ByteArray = d
                  as_data(b)
                }
            "#,
            check: DefaultFunction::UnBData,
            good: bytes(&[1]),
            returned: bytes(&[1]),
            bad: int(1),
        },
        Scenario {
            name: "List",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect xs: List<Data> = d
                  as_data(xs)
                }
            "#,
            check: DefaultFunction::UnListData,
            good: list(),
            returned: list(),
            bad: int(1),
        },
        Scenario {
            name: "Pairs",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect m: Pairs<Data, Data> = d
                  as_data(m)
                }
            "#,
            check: DefaultFunction::UnMapData,
            good: pairs(),
            returned: pairs(),
            bad: int(1),
        },
    ]
}

fn pair_accessor_expects() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "Int key",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect Pair(k, _): Pair<Int, Data> = d
                  as_data(k)
                }
            "#,
            check: DefaultFunction::UnIData,
            good: pair(int(1), int(2)),
            returned: int(1),
            bad: pair(bytes(&[0]), int(2)),
        },
        Scenario {
            name: "ByteArray value",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect Pair(_, v): Pair<Data, ByteArray> = d
                  as_data(v)
                }
            "#,
            check: DefaultFunction::UnBData,
            good: pair(int(1), bytes(&[2])),
            returned: bytes(&[2]),
            bad: pair(int(1), int(2)),
        },
        Scenario {
            name: "List key",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect Pair(k, _): Pair<List<Data>, Data> = d
                  as_data(k)
                }
            "#,
            check: DefaultFunction::UnListData,
            good: pair(list(), int(2)),
            returned: list(),
            bad: pair(int(1), int(2)),
        },
        Scenario {
            name: "Pairs value",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect Pair(_, v): Pair<Data, Pairs<Data, Data>> = d
                  as_data(v)
                }
            "#,
            check: DefaultFunction::UnMapData,
            good: pair(int(1), pairs()),
            returned: pairs(),
            bad: pair(int(1), int(2)),
        },
    ]
}

fn list_pattern_expects() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "Int head",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect [a, _]: List<Int> = d
                  as_data(a)
                }
            "#,
            check: DefaultFunction::UnIData,
            good: Data::list(vec![int(1), int(2)]),
            returned: int(1),
            bad: Data::list(vec![bytes(&[0]), int(2)]),
        },
        Scenario {
            name: "ByteArray second item",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect [_, b]: List<ByteArray> = d
                  as_data(b)
                }
            "#,
            check: DefaultFunction::UnBData,
            good: Data::list(vec![bytes(&[1]), bytes(&[2])]),
            returned: bytes(&[2]),
            bad: Data::list(vec![bytes(&[1]), int(2)]),
        },
        Scenario {
            name: "List head",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect [a, _]: List<List<Data>> = d
                  as_data(a)
                }
            "#,
            check: DefaultFunction::UnListData,
            good: Data::list(vec![list(), list()]),
            returned: list(),
            bad: Data::list(vec![int(1), list()]),
        },
        Scenario {
            name: "Pairs second item",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect [_, b]: List<Pairs<Data, Data>> = d
                  as_data(b)
                }
            "#,
            check: DefaultFunction::UnMapData,
            good: Data::list(vec![pairs(), pairs()]),
            returned: pairs(),
            bad: Data::list(vec![pairs(), int(2)]),
        },
    ]
}

#[test]
fn expect_then_upcast_keeps_the_check() {
    assert_check_kept(&plain_expects());
    assert_check_kept(&typed_list_items_expects());
    assert_trusted_casts_are_still_erased();
}

#[test]
fn pair_accessor_expect_then_upcast_keeps_the_check() {
    assert_check_kept(&pair_accessor_expects());
}

#[test]
fn list_pattern_expect_then_upcast_keeps_the_check() {
    assert_check_kept(&list_pattern_expects());
}

/// Casts that `expect` generates as part of checking a typed list: each item is cast on its
/// own, and a bad item must be rejected.
fn typed_list_items_expects() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "List<Int>",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect xs: List<Int> = d
                  as_data(xs)
                }
            "#,
            check: DefaultFunction::UnIData,
            good: Data::list(vec![int(1), int(2)]),
            returned: Data::list(vec![int(1), int(2)]),
            bad: Data::list(vec![int(1), bytes(&[0])]),
        },
        Scenario {
            name: "List<List<Data>>",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect xs: List<List<Data>> = d
                  as_data(xs)
                }
            "#,
            check: DefaultFunction::UnListData,
            good: Data::list(vec![list(), list()]),
            returned: Data::list(vec![list(), list()]),
            bad: Data::list(vec![list(), int(1)]),
        },
        Scenario {
            name: "Pairs<ByteArray, Int>",
            source: r#"
                pub fn observe(d: Data) -> Data {
                  expect xs: Pairs<ByteArray, Int> = d
                  as_data(xs)
                }
            "#,
            check: DefaultFunction::UnBData,
            good: Data::map(vec![(bytes(&[1]), int(2))]),
            returned: Data::map(vec![(bytes(&[1]), int(2))]),
            bad: Data::map(vec![(bytes(&[1]), int(2)), (int(3), int(4))]),
        },
    ]
}

fn all_checked_expects() -> Vec<Scenario> {
    let mut scenarios = plain_expects();
    scenarios.extend(pair_accessor_expects());
    scenarios.extend(list_pattern_expects());
    scenarios
}

/// What marks a cast is the unoptimized program; the optimizer takes the marker off again.
#[test]
fn expect_casts_are_marked_until_optimized() {
    let silent = Tracing::All(TraceLevel::Silent);

    for scenario in all_checked_expects() {
        let unoptimized = compile_unoptimized(scenario.source, silent);

        assert!(
            count_checked_casts(&unoptimized.term) > 0,
            "{}: the expect cast is not marked\n{}",
            scenario.name,
            unoptimized.to_pretty(),
        );

        let optimized = compile(scenario.source, silent);

        assert_eq!(count_checked_casts(&optimized.term), 0, "{}", scenario.name);
    }

    assert_other_casts_are_not_marked();
}

/// Every item of a typed list is cast (and checked) on its own: that makes two marked casts for
/// `List<Int>`, the list and its items, where `List<Data>` has the list alone.
#[test]
fn typed_list_items_casts_are_marked() {
    let silent = Tracing::All(TraceLevel::Silent);

    let marked = |tipo: &str| {
        count_checked_casts(
            &compile_unoptimized(
                &format!(
                    r#"
                    pub fn observe(d: Data) -> Data {{
                      expect xs: {tipo} = d
                      as_data(xs)
                    }}
                    "#
                ),
                silent,
            )
            .term,
        )
    };

    assert_eq!(marked("List<Data>"), 1);
    assert_eq!(marked("List<Int>"), 2);
}

/// Casts to types that no `xData` undoes, and trusted casts, are left as they were.
fn assert_other_casts_are_not_marked() {
    let silent = Tracing::All(TraceLevel::Silent);

    for (name, source) in [
        (
            "String",
            "pub fn observe(d: Data) -> Data { expect s: String = d  as_data(s) }",
        ),
        (
            "Bool",
            "pub fn observe(d: Data) -> Data { expect b: Bool = d  as_data(b) }",
        ),
        (
            "Void",
            "pub fn observe(d: Data) -> Data { expect u: Void = d  as_data(u) }",
        ),
        (
            "Option",
            "pub fn observe(d: Data) -> Data { expect o: Option<Data> = d  as_data(o) }",
        ),
        (
            "Pair",
            "pub fn observe(d: Data) -> Data { expect p: Pair<Data, Data> = d  as_data(p) }",
        ),
        (
            "trusted record field",
            r#"
            pub type Datum { amount: Int, owner: ByteArray }

            pub fn observe(datum: Datum) -> Data {
              as_data(datum.amount)
            }
            "#,
        ),
        (
            "trusted argument",
            "pub fn observe(n: Int) -> Data { as_data(n) }",
        ),
    ] {
        let program = compile_unoptimized(source, silent);

        assert_eq!(
            count_checked_casts(&program.term),
            0,
            "{name}: unexpected marker\n{}",
            program.to_pretty(),
        );
    }
}

/// Trusted data was already validated, so casting it into a type and back stays free.
fn assert_trusted_casts_are_still_erased() {
    for (name, source, input) in [
        (
            "argument",
            "pub fn observe(n: Int) -> Data { as_data(n) }",
            int(5),
        ),
        (
            "record field",
            r#"
            pub type Datum { amount: Int }

            pub fn observe(datum: Datum) -> Data {
              as_data(datum.amount)
            }
            "#,
            Data::constr(0, vec![int(5)]),
        ),
        (
            "list argument",
            "pub fn observe(xs: List<Data>) -> Data { as_data(xs) }",
            list(),
        ),
        (
            "pairs argument",
            "pub fn observe(xs: Pairs<Data, Data>) -> Data { as_data(xs) }",
            pairs(),
        ),
    ] {
        for tracing in TRACINGS {
            let program = compile(source, tracing);

            for check in [
                DefaultFunction::UnIData,
                DefaultFunction::UnBData,
                DefaultFunction::UnListData,
                DefaultFunction::UnMapData,
            ] {
                assert_eq!(
                    count_builtin(&program.term, check),
                    0,
                    "{name}: trusted cast was not erased\n{}",
                    program.to_pretty(),
                );
            }

            assert!(evaluate(&program, &input).is_ok(), "{name}");
        }
    }
}
