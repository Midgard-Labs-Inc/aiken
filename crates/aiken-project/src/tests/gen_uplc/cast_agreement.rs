//! A cast from `Data` accepts or rejects its input the same way whatever the compiler does
//! around it: with or without traces, and with or without the shrinker. Rejecting is how a
//! validator refuses a transaction, so an optimization or a trace level that changed the outcome
//! would change what a script accepts.
use super::*;
use aiken_lang::ast::TypedFunction;
use proptest::{
    prelude::*,
    test_runner::{Config, TestRng, TestRunner},
};
use uplc::{
    ast::NamedDeBruijn, machine::cost_model::ExBudget, optimize::interner::CodeGenInterner,
};

const TRACINGS: [Tracing; 2] = [
    Tracing::All(TraceLevel::Silent),
    Tracing::All(TraceLevel::Verbose),
];

/// Each source defines `observe(d: Data) -> Data`, which casts `d` and converts what it got back
/// into `Data`.
const SOURCES: &[(&str, &str)] = &[
    (
        "expect Int",
        r#"
        pub fn observe(d: Data) -> Data {
          expect n: Int = d
          as_data(n)
        }
        "#,
    ),
    (
        "expect ByteArray",
        r#"
        pub fn observe(d: Data) -> Data {
          expect b: ByteArray = d
          as_data(b)
        }
        "#,
    ),
    (
        "expect List<Int>",
        r#"
        pub fn observe(d: Data) -> Data {
          expect xs: List<Int> = d
          as_data(xs)
        }
        "#,
    ),
    (
        "expect Pairs<Int, ByteArray>",
        r#"
        pub fn observe(d: Data) -> Data {
          expect m: Pairs<Int, ByteArray> = d
          as_data(m)
        }
        "#,
    ),
    (
        "expect Pair key",
        r#"
        pub fn observe(d: Data) -> Data {
          expect Pair(k, _): Pair<Int, Data> = d
          as_data(k)
        }
        "#,
    ),
    (
        "expect list head",
        r#"
        pub fn observe(d: Data) -> Data {
          expect [a, ..]: List<ByteArray> = d
          as_data(a)
        }
        "#,
    ),
    (
        "expect record",
        r#"
        pub type Datum {
          Owner { owner: ByteArray, amount: Int }
          Nobody
        }

        pub fn observe(d: Data) -> Data {
          expect datum: Datum = d
          when datum is {
            Owner { amount, .. } -> as_data(amount)
            Nobody -> as_data(datum)
          }
        }
        "#,
    ),
    (
        "expect Value",
        r#"
        pub fn observe(d: Data) -> Data {
          expect v: Value = d
          as_data(v)
        }
        "#,
    ),
    (
        "expect G1Element",
        r#"
        pub fn observe(d: Data) -> Data {
          expect p: G1Element = d
          as_data(p)
        }
        "#,
    ),
    (
        "if is Int",
        r#"
        pub fn observe(d: Data) -> Data {
          if d is n: Int {
            as_data(n)
          } else {
            as_data(Void)
          }
        }
        "#,
    ),
    (
        "if is record",
        r#"
        pub type Datum {
          Owner { owner: ByteArray, amount: Int }
          Nobody
        }

        pub fn observe(d: Data) -> Data {
          if d is datum: Datum {
            as_data(datum)
          } else {
            as_data(Void)
          }
        }
        "#,
    ),
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

/// The program as the shrinker leaves it, and the same program with only the markers the
/// shrinker relies on removed.
fn compile(source: &str, tracing: Tracing) -> [Program<DeBruijn>; 2] {
    let mut project = TestProject::new();
    let (function, module) = observe(&mut project, source);

    let optimized = project
        .new_generator(tracing)
        .generate_raw(&function.body, &function.arguments, &module)
        .unwrap();

    let mut unoptimized = project
        .new_generator(tracing)
        .generate_raw_unoptimized(&function.body, &function.arguments, &module)
        .unwrap();
    CodeGenInterner::new().program(&mut unoptimized);
    let unoptimized = unoptimized.clean_up_no_inlines();

    [
        optimized.try_into().unwrap(),
        unoptimized.try_into().unwrap(),
    ]
}

fn evaluate(program: &Program<DeBruijn>, input: &Data) -> Result<Term<NamedDeBruijn>, ()> {
    program
        .clone()
        .apply_data(input.clone())
        .eval(ExBudget::max())
        .result()
        .map_err(|_| ())
}

fn arbitrary_data() -> impl Strategy<Value = Data> {
    let leaf = prop_oneof![
        (-3i64..300).prop_map(|n| Data::integer(n.into())),
        proptest::collection::vec(any::<u8>(), 0..4).prop_map(Data::bytestring),
        // A compressed G1 point: the generator, and the point at infinity.
        Just(Data::bytestring(G1_GENERATOR.to_vec())),
        Just(Data::bytestring(G1_INFINITY.to_vec())),
    ];

    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(Data::list),
            proptest::collection::vec((inner.clone(), inner.clone()), 0..3).prop_map(Data::map),
            (0u64..3, proptest::collection::vec(inner, 0..3))
                .prop_map(|(tag, fields)| Data::constr(tag, fields)),
            // A well-formed value: ascending policies and assets, non-zero quantities.
            proptest::collection::btree_map(
                proptest::collection::vec(any::<u8>(), 0..2),
                proptest::collection::btree_map(
                    proptest::collection::vec(any::<u8>(), 0..2),
                    prop_oneof![-5i64..0, 1i64..5],
                    1..3,
                ),
                0..3,
            )
            .prop_map(|policies| {
                Data::map(
                    policies
                        .into_iter()
                        .map(|(policy, assets)| {
                            (
                                Data::bytestring(policy),
                                Data::map(
                                    assets
                                        .into_iter()
                                        .map(|(asset, quantity)| {
                                            (
                                                Data::bytestring(asset),
                                                Data::integer(quantity.into()),
                                            )
                                        })
                                        .collect(),
                                ),
                            )
                        })
                        .collect(),
                )
            }),
        ]
    })
}

const G1_GENERATOR: [u8; 48] = [
    0x97, 0xf1, 0xd3, 0xa7, 0x31, 0x97, 0xd7, 0x94, 0x26, 0x95, 0x63, 0x8c, 0x4f, 0xa9, 0xac, 0x0f,
    0xc3, 0x68, 0x8c, 0x4f, 0x97, 0x74, 0xb9, 0x05, 0xa1, 0x4e, 0x3a, 0x3f, 0x17, 0x1b, 0xac, 0x58,
    0x6c, 0x55, 0xe8, 0x3f, 0xf9, 0x7a, 0x1a, 0xef, 0xfb, 0x3a, 0xf0, 0x0a, 0xdb, 0x22, 0xc6, 0xbb,
];

const G1_INFINITY: [u8; 48] = {
    let mut bytes = [0; 48];
    bytes[0] = 0xc0;
    bytes
};

/// An input each source accepts, and one it turns down (rejects, or sends to `else`).
fn fixtures(name: &str) -> [Data; 2] {
    let int = |n: i64| Data::integer(n.into());
    let bytes = |b: &[u8]| Data::bytestring(b.to_vec());

    match name {
        "expect Int" | "if is Int" => [int(42), bytes(&[0])],
        "expect ByteArray" => [bytes(&[1]), int(1)],
        "expect List<Int>" => [Data::list(vec![int(1)]), Data::list(vec![bytes(&[1])])],
        "expect Pairs<Int, ByteArray>" => [
            Data::map(vec![(int(1), bytes(&[2]))]),
            Data::map(vec![(bytes(&[2]), int(1))]),
        ],
        "expect Pair key" => [
            Data::list(vec![int(1), bytes(&[2])]),
            Data::list(vec![bytes(&[1]), bytes(&[2])]),
        ],
        "expect list head" => [Data::list(vec![bytes(&[1])]), Data::list(vec![int(1)])],
        "expect record" | "if is record" => [
            Data::constr(0, vec![bytes(&[1]), int(5)]),
            Data::constr(2, vec![]),
        ],
        "expect Value" => [
            Data::map(vec![(bytes(&[]), Data::map(vec![(bytes(&[]), int(5))]))]),
            Data::map(vec![(bytes(&[]), Data::map(vec![(bytes(&[]), int(0))]))]),
        ],
        "expect G1Element" => [bytes(&G1_GENERATOR), bytes(&[0])],
        _ => unreachable!("no fixtures for {name}"),
    }
}

#[test]
fn casts_accept_the_same_inputs_with_or_without_traces_and_optimizations() {
    let void = Ok(Term::data(Data::constr(0, vec![])));

    for (name, source) in SOURCES {
        let programs = TRACINGS.map(|tracing| compile(source, tracing));
        let [[silent, silent_unoptimized], [verbose, verbose_unoptimized]] = &programs;

        let variants = [
            ("silent", silent),
            ("verbose", verbose),
            ("verbose unoptimized", verbose_unoptimized),
        ];

        // Every variant accepts the first fixture and turns down the second.
        let [accepted, turned_down] = fixtures(name);

        for (variant, program) in variants
            .iter()
            .copied()
            .chain([("silent unoptimized", silent_unoptimized)])
        {
            let result = evaluate(program, &accepted);
            assert!(
                result.is_ok() && result != void,
                "{name}: {variant} doesn't accept {accepted:?}"
            );

            let result = evaluate(program, &turned_down);
            assert!(
                result.is_err() || result == void,
                "{name}: {variant} doesn't turn down {turned_down:?}"
            );
        }

        let config = Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        };
        let rng = TestRng::deterministic_rng(config.rng_algorithm);

        TestRunner::new_with_rng(config, rng)
            .run(&arbitrary_data(), |input| {
                let reference = evaluate(silent_unoptimized, &input);

                for (variant, program) in variants {
                    prop_assert_eq!(
                        evaluate(program, &input),
                        reference.clone(),
                        "{}: {} disagrees with silent unoptimized on {:?}",
                        name,
                        variant,
                        input
                    );
                }

                Ok(())
            })
            .unwrap_or_else(|error| panic!("{error}"));
    }
}

/// `x == d` with `d: Data` upcasts `x`, whichever side it is on: both are compared as Data.
#[test]
fn equality_with_data_compares_as_data_in_both_orders() {
    let bool_data = |b: bool| Ok(Term::data(Data::constr(u64::from(b), vec![])));
    let int = |n: i64| Data::integer(n.into());

    for (name, value, equal, different) in [
        ("Int", "1", int(1), int(2)),
        ("ByteArray", "#\"01\"", Data::bytestring(vec![1]), int(1)),
        (
            "Bool",
            "True",
            Data::constr(1, vec![]),
            Data::constr(0, vec![]),
        ),
        (
            "G1Element",
            "#<Bls12_381, G1>\"97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb\"",
            Data::bytestring(G1_GENERATOR.to_vec()),
            Data::bytestring(G1_INFINITY.to_vec()),
        ),
        (
            "Value",
            "{ lovelace: 5 }",
            Data::map(vec![(
                Data::bytestring(vec![]),
                Data::map(vec![(Data::bytestring(vec![]), int(5))]),
            )]),
            int(5),
        ),
    ] {
        for (order, comparison) in [("x == d", "x == d"), ("d == x", "d == x")] {
            for (op, negate) in [("==", false), ("!=", true)] {
                let comparison = comparison.replace("==", op);
                let source = format!(
                    "pub fn observe(d: Data) -> Data {{\n  let x: {name} = {value}\n  as_data({comparison})\n}}"
                );

                for tracing in TRACINGS {
                    for program in compile(&source, tracing) {
                        for (input, expected) in [(&equal, true), (&different, false)] {
                            pretty_assertions::assert_eq!(
                                evaluate(&program, input),
                                bool_data(expected != negate),
                                "{name}, {order} with {op}, {tracing:?}, on {input:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
