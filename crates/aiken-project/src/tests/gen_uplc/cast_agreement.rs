//! A cast from `Data` accepts or rejects its input the same way whatever the compiler does
//! around it: with or without traces, and with or without the shrinker. Rejecting is how a
//! validator refuses a transaction, so an optimization or a trace level that changed the outcome
//! would change what a script accepts.
use super::*;
use aiken_lang::ast::TypedFunction;
use uplc::{
    ast::NamedDeBruijn, machine::cost_model::ExBudget, optimize::interner::CodeGenInterner,
};

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
