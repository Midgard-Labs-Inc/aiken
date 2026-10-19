//! Cast holes that are still open. Each test states the outcome that
//! `site/src/content/docs/casts.mdx` prescribes, fails on the current compiler, and is ignored
//! until the hole is fixed: run them with `cargo test -p aiken-project open_cast_holes -- --ignored`.
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

    project
        .new_generator(tracing)
        .generate(&validator, &module.name)
        .unwrap()
        .try_into()
        .unwrap()
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

/// casts.mdx: "A validator's parameters, datum and redeemer are converted from Data too". The
/// redeemer goes through `expect`; parameters go through `cast_validator_args`, which only applies
/// `known_data_to_type`: a record parameter is never checked, a Bool parameter is
/// `fst (unConstrData d) == 1`, a List<Int> parameter's items are never checked.
#[test]
#[ignore = "open hole: validator parameters are converted with known_data_to_type (cast_validator_args), so their shape is never checked"]
fn validator_parameters_are_checked_like_a_redeemer() {
    let source = r#"
        pub type Config {
          Config { owner: ByteArray, limit: Int }
        }

        validator params(cfg: Config, flag: Bool, xs: List<Int>) {
          else(_ctx) {
            flag
          }
        }
    "#;

    let cfg = Data::constr(0, vec![Data::bytestring(vec![0]), int(1)]);
    let flag = Data::constr(1, vec![]);
    let xs = Data::list(vec![int(1)]);
    let ctx = int(0);

    for tracing in TRACINGS {
        let program = compile_validator(source, tracing);

        assert!(
            evaluate(
                &program,
                &[cfg.clone(), flag.clone(), xs.clone(), ctx.clone()]
            )
            .is_ok(),
            "{tracing:?}: well-formed parameters must be accepted"
        );

        for (what, args) in [
            (
                "a Config parameter that is an integer",
                [int(5), flag.clone(), xs.clone(), ctx.clone()],
            ),
            (
                "a Bool parameter with a field",
                [
                    cfg.clone(),
                    Data::constr(1, vec![int(5)]),
                    xs.clone(),
                    ctx.clone(),
                ],
            ),
            (
                "a List<Int> parameter holding bytes",
                [
                    cfg.clone(),
                    flag.clone(),
                    Data::list(vec![Data::bytestring(vec![0])]),
                    ctx.clone(),
                ],
            ),
        ] {
            assert!(
                evaluate(&program, &args).is_err(),
                "{tracing:?}: the validator accepts {what}"
            );
        }
    }
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
        let program = compile_validator(source, tracing);

        assert!(
            evaluate(
                &program,
                &[context(Data::constr(0, vec![owner.clone(), int(1)]))]
            )
            .is_ok(),
            "{tracing:?}: a well-formed datum must be accepted"
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
                "{tracing:?}: the validator accepts a datum with {what}"
            );
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
        let program = compile_validator(source, tracing);

        assert!(
            evaluate(&program, &[context(int(5), 1)]).is_ok(),
            "{tracing:?}: a well-formed redeemer must be accepted"
        );
        assert!(
            evaluate(&program, &[context(Data::bytestring(vec![0xaa]), 1)]).is_err(),
            "{tracing:?}: the validator accepts a redeemer that is not an Int"
        );
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
