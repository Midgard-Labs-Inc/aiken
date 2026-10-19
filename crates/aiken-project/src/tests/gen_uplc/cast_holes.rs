//! Cast holes: programs where a cast through Data, or the optimizer around it, didn't behave as
//! `site/src/content/docs/casts.mdx` prescribes. Each test states the prescribed outcome, at
//! every trace level, on the optimized program and on the unoptimized one where it can run.
use super::*;
use aiken_lang::ast::{TypedFunction, TypedValidator};
use uplc::{
    ast::NamedDeBruijn, machine::cost_model::ExBudget, optimize::interner::CodeGenInterner,
};

const TRACINGS: [Tracing; 2] = [
    Tracing::All(TraceLevel::Silent),
    Tracing::All(TraceLevel::Verbose),
];

fn function(project: &mut TestProject, source: &str, name: &str) -> (TypedFunction, String) {
    let module = project.check(project.parse(source));

    let function = module
        .ast
        .definitions()
        .find_map(|definition| match definition {
            Definition::Fn(function) if function.name == name => Some(function.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("source should define a function named {name}"));

    (function, module.name.clone())
}

/// `observe` compiled as the shrinker leaves it, and with only the markers the shrinker relies on
/// removed.
fn compile_observe(source: &str, tracing: Tracing) -> [(&'static str, Program<DeBruijn>); 2] {
    let mut project = TestProject::new();
    let (observe, module) = function(&mut project, source, "observe");

    let optimized = project
        .new_generator(tracing)
        .generate_raw(&observe.body, &observe.arguments, &module)
        .unwrap();

    let mut unoptimized = project
        .new_generator(tracing)
        .generate_raw_unoptimized(&observe.body, &observe.arguments, &module)
        .unwrap();
    CodeGenInterner::new().program(&mut unoptimized);
    let unoptimized = unoptimized.clean_up_no_inlines();

    [
        ("optimized", optimized.try_into().unwrap()),
        ("unoptimized", unoptimized.try_into().unwrap()),
    ]
}

fn compile_validator(source: &str, tracing: Tracing) -> Program<DeBruijn> {
    let [(_, optimized), _] = compile_validator_builds(source, tracing);
    optimized
}

/// The validator compiled as the shrinker leaves it, and with only the markers the shrinker
/// relies on removed, like `compile_observe`.
fn compile_validator_builds(
    source: &str,
    tracing: Tracing,
) -> [(&'static str, Program<DeBruijn>); 2] {
    let mut project = TestProject::new();
    let module = project.check(project.parse(source));

    let validator: TypedValidator = module
        .ast
        .definitions()
        .find_map(|definition| match definition {
            Definition::Validator(validator) => Some(validator.clone()),
            _ => None,
        })
        .expect("source should define a validator");

    let optimized = project
        .new_generator(tracing)
        .generate(&validator, &module.name)
        .unwrap();

    let mut unoptimized = project
        .new_generator(tracing)
        .generate_unoptimized(&validator, &module.name)
        .unwrap();
    CodeGenInterner::new().program(&mut unoptimized);
    let unoptimized = unoptimized.clean_up_no_inlines();

    [
        ("optimized", optimized.try_into().unwrap()),
        ("unoptimized", unoptimized.try_into().unwrap()),
    ]
}

fn evaluate(program: &Program<DeBruijn>, args: &[Data]) -> Result<Term<NamedDeBruijn>, ()> {
    args.iter()
        .fold(program.clone(), |program, arg| {
            program.apply_data(arg.clone())
        })
        .eval(ExBudget::max())
        .result()
        .map_err(|_| ())
}

fn int(n: i64) -> Data {
    Data::integer(n.into())
}

/// The spend handler's datum is an `Option<Data>` in the script context. The handler wrapper
/// (`into_script_context_handler`) must check it against the declared `Option<T>`, inner `T`
/// included, like the redeemer.
#[test]
fn spend_datum_is_checked_like_a_redeemer() {
    let source = r#"
        pub type MyDatum {
          owner: ByteArray,
          amount: Int,
        }

        validator datum_shape {
          spend(datum: Option<MyDatum>, _redeemer: Data, _own_ref: Data, _self: Data) {
            expect Some(d) = datum
            d.owner == #"00"
          }

          else(_) {
            fail
          }
        }
    "#;

    // ScriptContext { transaction, redeemer, info: Spending { output, datum: Some(datum) } }
    let context = |datum: Data| {
        Data::constr(
            0,
            vec![
                int(0),
                int(0),
                Data::constr(1, vec![int(0), Data::constr(0, vec![datum])]),
            ],
        )
    };

    let owner = Data::bytestring(vec![0]);

    for tracing in TRACINGS {
        for (build, program) in compile_validator_builds(source, tracing) {
            assert!(
                evaluate(
                    &program,
                    &[context(Data::constr(0, vec![owner.clone(), int(1)]))]
                )
                .is_ok(),
                "{tracing:?}, {build}: a well-formed datum must be accepted"
            );

            for (what, datum) in [
                ("a missing field", Data::constr(0, vec![owner.clone()])),
                (
                    "an amount that is bytes",
                    Data::constr(0, vec![owner.clone(), Data::bytestring(vec![0xff])]),
                ),
                (
                    "an extra field",
                    Data::constr(0, vec![owner.clone(), int(1), int(2)]),
                ),
            ] {
                assert!(
                    evaluate(&program, &[context(datum)]).is_err(),
                    "{tracing:?}, {build}: the validator accepts a datum with {what}"
                );
            }
        }
    }
}

/// A record-level `@tag(5)` makes construction (`get_constr_index_variant`) and the blueprint use
/// constructor index 5, so `expect` and `if … is` must check index 5, not 0.
#[test]
fn expect_checks_the_index_a_record_level_tag_gives() {
    let tagged = Data::constr(5, vec![int(1)]);
    let untagged = Data::constr(0, vec![int(1)]);
    let sentinel = Data::bytestring(vec![0x5e]);

    let expect = r#"
        @tag(5)
        pub type R {
          a: Int,
        }

        pub fn observe(d: Data) -> Data {
          expect r: R = d
          as_data(r)
        }
    "#;

    let soft = r#"
        @tag(5)
        pub type R {
          a: Int,
        }

        pub fn observe(d: Data) -> Data {
          if d is r: R {
            as_data(r)
          } else {
            as_data(#"5e")
          }
        }
    "#;

    let turned_down = [Err(()), Ok(Term::data(sentinel))];

    for tracing in TRACINGS {
        for ((cast, source), turned_down) in [("expect", expect), ("if/is", soft)]
            .into_iter()
            .zip(turned_down.iter())
        {
            for (build, program) in compile_observe(source, tracing) {
                pretty_assertions::assert_eq!(
                    evaluate(&program, std::slice::from_ref(&tagged)),
                    Ok(Term::data(tagged.clone())),
                    "{cast}, {tracing:?}, {build}: the Data of R {{ a: 1 }} is turned down"
                );
                pretty_assertions::assert_eq!(
                    &evaluate(&program, std::slice::from_ref(&untagged)),
                    turned_down,
                    "{cast}, {tracing:?}, {build}: index 0 is accepted"
                );
            }
        }
    }
}

/// `carry_args_to_branch` treats any occurrence inside a delayed `ifThenElse` branch as executed,
/// so `inline_reducer` moves the check of an `expect` into the one branch that uses its result.
/// With traces the check has another shape and stays put, so silent and verbose builds disagree.
#[test]
fn expect_check_runs_before_a_branch_that_uses_its_result() {
    let source = r#"
        pub fn observe(d: Data) -> Data {
          expect n: Int = d
          if d == as_data(0) {
            as_data(n + 1)
          } else {
            as_data(7)
          }
        }
    "#;

    for tracing in TRACINGS {
        for (build, program) in compile_observe(source, tracing) {
            pretty_assertions::assert_eq!(
                evaluate(&program, &[int(0)]),
                Ok(Term::data(int(1))),
                "{tracing:?}, {build}: expect Int must accept I 0"
            );
            pretty_assertions::assert_eq!(
                evaluate(&program, &[Data::bytestring(vec![0xaa])]),
                Err(()),
                "{tracing:?}, {build}: expect Int accepts B #aa"
            );
        }
    }
}

/// For the casts whose conversion builtin is also their check (`decodeUtf8`, `unValueData`,
/// `bls12_381_G1_uncompress`) the same move happens with traces on: no trace level keeps the
/// check of the casts C made legal.
#[test]
fn conversion_check_runs_before_a_branch_at_every_trace_level() {
    let zero_quantity = Data::map(vec![(
        Data::bytestring(vec![]),
        Data::map(vec![(Data::bytestring(vec![]), int(0))]),
    )]);

    for (tipo, malformed) in [
        ("String", Data::bytestring(vec![0xff])),
        ("Value", zero_quantity),
        ("G1Element", Data::bytestring(vec![0])),
    ] {
        let source = format!(
            r#"
            pub fn observe(d: Data) -> Data {{
              expect x: {tipo} = d
              if d == as_data(0) {{
                as_data(x)
              }} else {{
                as_data(7)
              }}
            }}
            "#
        );

        // Verbose first: that's the trace level the other holes of this kind spare.
        for tracing in TRACINGS.into_iter().rev() {
            for (build, program) in compile_observe(&source, tracing) {
                pretty_assertions::assert_eq!(
                    evaluate(&program, std::slice::from_ref(&malformed)),
                    Err(()),
                    "{tipo}, {tracing:?}, {build}: expect accepts {malformed:?}"
                );
            }
        }
    }
}

/// The same hole on the `expect` the handler wrapper emits for a typed redeemer: at `-t silent`
/// (the default of `aiken build`) the redeemer check only runs on the branch that reads it.
#[test]
fn redeemer_check_runs_before_a_branch_that_reads_it() {
    let source = r#"
        validator typed_redeemer {
          mint(redeemer: Int, policy_id: ByteArray, _self: Data) {
            if policy_id == #"00" {
              redeemer == 1
            } else {
              True
            }
          }

          else(_) {
            fail
          }
        }
    "#;

    // ScriptContext { transaction, redeemer, info: Minting(policy_id) }
    let context = |redeemer: Data, policy_id: u8| {
        Data::constr(
            0,
            vec![
                int(0),
                redeemer,
                Data::constr(0, vec![Data::bytestring(vec![policy_id])]),
            ],
        )
    };

    for tracing in TRACINGS {
        for (build, program) in compile_validator_builds(source, tracing) {
            assert!(
                evaluate(&program, &[context(int(5), 1)]).is_ok(),
                "{tracing:?}, {build}: a well-formed redeemer must be accepted"
            );
            assert!(
                evaluate(&program, &[context(Data::bytestring(vec![0xaa]), 1)]).is_err(),
                "{tracing:?}, {build}: the validator accepts a redeemer that is not an Int"
            );
        }
    }
}

/// `split_body_lambda` moves an applied argument under an unapplied lambda: the check of an
/// `expect` that precedes a returned closure only runs if the closure is called.
#[test]
fn expect_check_runs_before_the_closure_it_precedes() {
    let source = r#"
        fn ignore(_f: fn(Int) -> Int) -> Int {
          1
        }

        pub fn observe(d: Data) -> Data {
          let f = {
            expect n: Int = d
            fn(x) { x + n }
          }
          as_data(ignore(f))
        }
    "#;

    for tracing in TRACINGS {
        for (build, program) in compile_observe(source, tracing) {
            pretty_assertions::assert_eq!(
                evaluate(&program, &[int(5)]),
                Ok(Term::data(int(1))),
                "{tracing:?}, {build}: expect Int must accept I 5"
            );
            pretty_assertions::assert_eq!(
                evaluate(&program, &[Data::bytestring(vec![0xaa])]),
                Err(()),
                "{tracing:?}, {build}: expect Int accepts B #aa"
            );
        }
    }
}

/// Three sites sharing a non-constant first argument: `builtin_curry_reducer` must not hoist
/// `sliceByteString i 2`, whose prefix variable it never defines (the optimizer panicked with
/// `FreeUnique`).
#[test]
fn three_slices_with_a_shared_variable_start_compile() {
    let source = r#"
        use aiken/builtin

        validator slices {
          mint(i: Int, _policy_id: ByteArray, _self: Data) {
            let a = builtin.slice_bytearray(i, 2, #"aabbcc")
            let b = builtin.slice_bytearray(i, 2, #"001122")
            let c = builtin.slice_bytearray(i, 2, #"ddeeff")
            a != b && b != c
          }

          else(_) {
            fail
          }
        }
    "#;

    let context = Data::constr(
        0,
        vec![
            int(0),
            int(0),
            Data::constr(0, vec![Data::bytestring(vec![0])]),
        ],
    );

    for tracing in TRACINGS {
        let program = compile_validator(source, tracing);

        assert!(
            evaluate(&program, std::slice::from_ref(&context)).is_ok(),
            "{tracing:?}: three different slices must be accepted"
        );
    }
}

/// The checked `unListData` of an `expect` on constant Data is folded and inlined into
/// `writeBits`: `typed_list_convert_arg` must leave the malformed constant list to the runtime
/// conversion, so the program fails as the `expect` would, instead of panicking.
#[test]
fn expect_of_a_malformed_constant_list_into_write_bits_fails_at_runtime() {
    let source = r#"
        use aiken/builtin

        pub fn observe(_d: Data) -> Data {
          let d: Data = [#"aa"]
          expect xs: List<Int> = d
          as_data(builtin.write_bits(#"00", xs, True))
        }
    "#;

    for tracing in TRACINGS {
        let mut project = TestProject::new();
        let (observe, module) = function(&mut project, source, "observe");

        let program: Program<DeBruijn> = project
            .new_generator(tracing)
            .generate_raw(&observe.body, &observe.arguments, &module)
            .unwrap()
            .try_into()
            .unwrap();

        pretty_assertions::assert_eq!(
            evaluate(&program, &[int(0)]),
            Err(()),
            "{tracing:?}: expect List<Int> accepts [B #aa]"
        );
    }
}

/// casts.mdx erases an "opaque type with one constructor, one field and no decorator". A `@tag`
/// on the constructor is a decorator too: the type keeps its constructor, and its documented
/// Data form is accepted.
#[test]
fn opaque_type_with_a_tagged_constructor_keeps_its_constructor() {
    let source = r#"
        pub opaque type W {
          @tag(3)
          W(Int)
        }

        fn get(w: W) -> Int {
          let W(n) = w
          n
        }

        pub fn observe(d: Data) -> Data {
          coerce w: W = d
          as_data(get(w))
        }
    "#;

    let documented = Data::constr(3, vec![int(5)]);

    for tracing in TRACINGS {
        for (build, program) in compile_observe(source, tracing) {
            pretty_assertions::assert_eq!(
                evaluate(&program, std::slice::from_ref(&documented)),
                Ok(Term::data(int(5))),
                "{tracing:?}, {build}: the documented Data form of W(5) is refused"
            );
        }
    }
}

/// A decorated opaque type keeps its constructor in nested patterns too: the decision tree reads
/// the field of `Some(V { inner })` instead of taking the whole constructor for it.
#[test]
fn decorated_opaque_type_keeps_its_constructor_in_a_nested_pattern() {
    for (decl, index) in [
        ("@tag(5)\npub opaque type V { inner: Int }", 5),
        ("pub opaque type V {\n  @tag(3)\n  V { inner: Int }\n}", 3),
    ] {
        let source = format!(
            r#"
            {decl}

            pub fn observe(d: Data) -> Data {{
              coerce o: Option<V> = d
              when o is {{
                Some(V {{ inner }}) -> as_data(inner)
                None -> as_data(0)
              }}
            }}
            "#
        );

        let some_v = Data::constr(0, vec![Data::constr(index, vec![int(5)])]);

        for tracing in TRACINGS {
            for (build, program) in compile_observe(&source, tracing) {
                pretty_assertions::assert_eq!(
                    evaluate(&program, std::slice::from_ref(&some_v)),
                    Ok(Term::data(int(5))),
                    "{decl}, {tracing:?}, {build}: Some(V {{ inner: 5 }}) doesn't give 5"
                );
            }
        }
    }
}

/// An erased opaque type is its field at runtime, and a literal pattern inside its constructor
/// tests that field: the decision tree used to crash on it ("all rows have the same number of
/// columns"), at the top of a `when` and nested in another pattern.
#[test]
fn erased_opaque_type_matches_a_literal_inside_its_constructor() {
    let declaration = "pub opaque type U { U(Int) }";

    for (body, cases) in [
        (
            "coerce u: U = d\n  when u is {\n    U(5) -> 1\n    U(6) -> 2\n    _ -> 0\n  }",
            vec![(int(5), 1), (int(6), 2), (int(7), 0)],
        ),
        (
            "coerce o: Option<U> = d\n  when o is {\n    Some(U(5)) -> 1\n    Some(_) -> 2\n    None -> 0\n  }",
            vec![
                (Data::constr(0, vec![int(5)]), 1),
                (Data::constr(0, vec![int(4)]), 2),
                (Data::constr(1, vec![]), 0),
            ],
        ),
    ] {
        let source = format!(
            "{declaration}\n\npub fn observe(d: Data) -> Data {{\n  let n = {{\n  {body}\n  }}\n  as_data(n)\n}}\n"
        );

        for tracing in TRACINGS {
            for (build, program) in compile_observe(&source, tracing) {
                for (input, expected) in &cases {
                    pretty_assertions::assert_eq!(
                        evaluate(&program, std::slice::from_ref(input)),
                        Ok(Term::data(int(*expected))),
                        "{body}\n{tracing:?}, {build}: {input:?}"
                    );
                }
            }
        }
    }
}

/// `expect xs: List<_>` doesn't know its element type yet. It learns `Int` from the next line, and
/// the check the `expect` compiles to is the one `expect xs: List<Int>` has: it accepts a list of
/// integers and refuses a list that holds anything else.
#[test]
fn expect_of_an_unknown_type_argument_checks_the_type_it_becomes() {
    let source = r#"
        pub fn observe(d: Data) -> Data {
          expect xs: List<_> = d
          let ys: List<Int> = xs
          as_data(ys)
        }
    "#;

    let ints = Data::list(vec![int(1), int(2)]);

    for tracing in TRACINGS {
        for (build, program) in compile_observe(source, tracing) {
            pretty_assertions::assert_eq!(
                evaluate(&program, std::slice::from_ref(&ints)),
                Ok(Term::data(ints.clone())),
                "{tracing:?}, {build}: expect List<Int> must accept {ints:?}"
            );

            for malformed in [
                Data::list(vec![int(1), Data::bytestring(vec![0xaa])]),
                Data::list(vec![Data::bytestring(vec![0xaa])]),
                Data::bytestring(vec![0xaa]),
            ] {
                pretty_assertions::assert_eq!(
                    evaluate(&program, std::slice::from_ref(&malformed)),
                    Err(()),
                    "{tracing:?}, {build}: expect List<Int> accepts {malformed:?}"
                );
            }
        }
    }
}

/// A capture, or a lambda bound by `let`, whose body upcasts its argument takes the type of the
/// higher-order function it's passed to, and converts with that type: `iData` here.
#[test]
fn captures_that_upcast_their_argument_convert_it_with_the_type_it_becomes() {
    let helpers = r#"
        use aiken/builtin

        fn eq_data(d: Data, n: Int) -> Bool {
          d == as_data(n)
        }

        fn serialise(d: Data) -> ByteArray {
          builtin.serialise_data(d)
        }

        fn both(xs: List<a>, f: fn(a) -> Bool) -> Bool {
          when xs is {
            [x, y] -> f(x) && f(y)
            _ -> False
          }
        }

        fn map_two(xs: List<a>, f: fn(a) -> b) -> List<b> {
          when xs is {
            [x, y] -> [f(x), f(y)]
            _ -> []
          }
        }

        fn apply(f: fn(Int) -> Bool, n: Int) -> Bool {
          f(n)
        }
    "#;

    for body in [
        "both([n, n], eq_data(_, 1))",
        "apply(eq_data(_, 1), n)",
        "both([n, n], fn(x) { eq_data(x, 1) })",
        "{ let is_one = fn(x) { eq_data(x, 1) }\n both([n, n], is_one) }",
    ] {
        let source = format!(
            "{helpers}\npub fn observe(d: Data) -> Bool {{\n  expect n: Int = d\n  {body}\n}}\n"
        );

        for tracing in TRACINGS {
            for (build, program) in compile_observe(&source, tracing) {
                pretty_assertions::assert_eq!(
                    evaluate(&program, &[int(1)]),
                    Ok(Term::bool(true)),
                    "{body}\n{tracing:?}, {build}: 1"
                );
                pretty_assertions::assert_eq!(
                    evaluate(&program, &[int(2)]),
                    Ok(Term::bool(false)),
                    "{body}\n{tracing:?}, {build}: 2"
                );
            }
        }
    }

    let source = format!(
        "{helpers}\npub fn observe(d: Data) -> Data {{\n  expect n: Int = d\n  as_data(map_two([n, 2], serialise(_)))\n}}\n"
    );

    for tracing in TRACINGS {
        for (build, program) in compile_observe(&source, tracing) {
            pretty_assertions::assert_eq!(
                evaluate(&program, &[int(1)]),
                Ok(Term::data(Data::list(vec![
                    Data::bytestring(vec![1]),
                    Data::bytestring(vec![2]),
                ]))),
                "serialise capture\n{tracing:?}, {build}"
            );
        }
    }
}

/// A record update builds the record's Data form, so it keeps the `@tag` the type, or its
/// constructor, carries: it used to build `Constr 0` whatever the tag, and the record it returned
/// failed every check of that type.
#[test]
fn record_update_keeps_the_tag_of_the_record() {
    let bytes = || Data::bytestring(vec![0]);

    for (decl, index) in [
        ("@tag(5)\npub type R { a: Int, b: ByteArray }", 5),
        (
            "pub type R {\n  @tag(6)\n  R { a: Int, b: ByteArray }\n}",
            6,
        ),
    ] {
        let source = format!(
            r#"
            {decl}

            pub fn observe(d: Data) -> Data {{
              expect r: R = d
              let updated = R {{ ..r, a: 3 }}
              as_data(updated)
            }}
            "#
        );

        for tracing in TRACINGS {
            for (build, program) in compile_observe(&source, tracing) {
                pretty_assertions::assert_eq!(
                    evaluate(&program, &[Data::constr(index, vec![int(1), bytes()])]),
                    Ok(Term::data(Data::constr(index, vec![int(3), bytes()]))),
                    "{decl}, {tracing:?}, {build}: the update doesn't build the Data of R"
                );
            }
        }
    }
}

/// A state machine that updates its tagged datum demands the well-formed next datum, so the output
/// it produces can be spent in turn.
#[test]
fn state_machine_with_a_tagged_datum_demands_the_well_formed_next_datum() {
    let source = r#"
        @tag(5)
        pub type State {
          count: Int,
          owner: ByteArray,
        }

        validator sm {
          spend(datum: Option<State>, redeemer: Data, _own_ref: Data, _self: Data) {
            expect Some(s) = datum
            as_data(State { ..s, count: s.count + 1 }) == redeemer
          }

          else(_) {
            fail
          }
        }
    "#;

    let state =
        |tag: u64, count: i64| Data::constr(tag, vec![int(count), Data::bytestring(vec![0])]);

    // ScriptContext { transaction, redeemer, info: Spending { output, datum: Some(datum) } }
    let context = |datum: Data, redeemer: Data| {
        Data::constr(
            0,
            vec![
                int(0),
                redeemer,
                Data::constr(1, vec![int(0), Data::constr(0, vec![datum])]),
            ],
        )
    };

    for tracing in TRACINGS {
        let program = compile_validator(source, tracing);

        assert!(
            evaluate(&program, &[context(state(5, 1), state(5, 2))]).is_ok(),
            "{tracing:?}: the well-formed next datum is turned down"
        );
        assert!(
            evaluate(&program, &[context(state(5, 1), state(0, 2))]).is_err(),
            "{tracing:?}: the malformed next datum is accepted"
        );
    }
}
