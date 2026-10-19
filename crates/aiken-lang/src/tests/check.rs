use crate::{
    IdGenerator,
    ast::{
        Definition, ModuleKind, Pattern, TraceLevel, Tracing, TypedModule, UntypedModule,
        UntypedPattern,
    },
    builtins,
    expr::{CallArg, Span, TypedExpr},
    parser,
    tipo::error::{Error, UnifyErrorSituation, Warning},
};
use std::collections::HashMap;

const DEFAULT_MODULE_NAME: &str = "my_module";
const DEFAULT_PACKAGE: &str = "test/project";

fn parse(source_code: &str) -> UntypedModule {
    parse_as(source_code, DEFAULT_MODULE_NAME)
}

fn parse_as(source_code: &str, name: &str) -> UntypedModule {
    let kind = ModuleKind::Lib;
    let (mut ast, _) = parser::module(source_code, kind).expect("Failed to parse module");
    ast.name = name.to_string();
    ast
}

#[allow(clippy::result_large_err)]
fn check_module(
    ast: UntypedModule,
    extra: Vec<UntypedModule>,
    kind: ModuleKind,
    tracing: Tracing,
) -> Result<(Vec<Warning>, TypedModule), (Vec<Warning>, Error)> {
    let id_gen = IdGenerator::new();

    let mut warnings = vec![];

    let mut module_types = HashMap::new();
    module_types.insert(builtins::PRELUDE.to_string(), builtins::prelude(&id_gen));
    module_types.insert(builtins::BUILTIN.to_string(), builtins::plutus(&id_gen));

    for module in extra {
        let mut warnings = vec![];

        if module.name == DEFAULT_MODULE_NAME {
            panic!(
                "passed extra modules with default name! Use 'parse_as' to define tests instead of 'parse'."
            );
        }

        let module_kind = module.kind;

        let typed_module = module
            .infer(
                &id_gen,
                module_kind,
                DEFAULT_PACKAGE,
                &module_types,
                Tracing::All(TraceLevel::Verbose),
                &mut warnings,
                None,
            )
            .expect("extra dependency did not compile");

        module_types.insert(typed_module.name.clone(), typed_module.type_info.clone());
    }

    let result = ast.infer(
        &id_gen,
        kind,
        DEFAULT_PACKAGE,
        &module_types,
        tracing,
        &mut warnings,
        None,
    );

    result
        .map(|o| (warnings.clone(), o))
        .map_err(|e| (warnings, e))
}

#[allow(clippy::result_large_err)]
fn check(ast: UntypedModule) -> Result<(Vec<Warning>, TypedModule), (Vec<Warning>, Error)> {
    check_module(ast, Vec::new(), ModuleKind::Lib, Tracing::verbose())
}

#[allow(clippy::result_large_err)]
fn check_with_verbosity(
    ast: UntypedModule,
    level: TraceLevel,
) -> Result<(Vec<Warning>, TypedModule), (Vec<Warning>, Error)> {
    check_module(ast, Vec::new(), ModuleKind::Lib, Tracing::All(level))
}

#[allow(clippy::result_large_err)]
fn check_with_deps(
    ast: UntypedModule,
    extra: Vec<UntypedModule>,
) -> Result<(Vec<Warning>, TypedModule), (Vec<Warning>, Error)> {
    check_module(ast, extra, ModuleKind::Lib, Tracing::verbose())
}

#[allow(clippy::result_large_err)]
fn check_validator(
    ast: UntypedModule,
) -> Result<(Vec<Warning>, TypedModule), (Vec<Warning>, Error)> {
    check_module(ast, Vec::new(), ModuleKind::Validator, Tracing::verbose())
}

#[allow(clippy::result_large_err)]
fn check_validator_with_deps(
    ast: UntypedModule,
    extra: Vec<UntypedModule>,
) -> Result<(Vec<Warning>, TypedModule), (Vec<Warning>, Error)> {
    check_module(ast, extra, ModuleKind::Validator, Tracing::verbose())
}

#[test]
fn bls12_381_elements_in_data_type() {
    let source_code = r#"
        type Datum {
          D0(G1Element)
          D1(G2Element)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn bls12_381_ml_result_in_data_type() {
    let source_code = r#"
        type Datum {
          thing: MillerLoopResult
        }
    "#;

    let res = check(parse(source_code));

    assert!(matches!(res, Err((_, Error::IllegalTypeInData { .. }))))
}

#[test]
fn validator_illegal_return_type() {
    let source_code = r#"
      validator foo {
        spend(d, r, c) -> Int {
          1
        }
      }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::ValidatorMustReturnBool { .. }))
    ))
}

#[test]
fn implicitly_discard_void() {
    let source_code = r#"
      pub fn label(str: String) -> Void {
        trace str Void
      }
    "#;

    let (warnings, _) = check_validator(parse(source_code)).expect("should type-check");

    assert!(warnings.is_empty(), "no warnings: {warnings:#?}");
}

#[test]
fn validator_illegal_arity() {
    let source_code = r#"
      validator foo {
        mint(c) {
          True
        }
      }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IncorrectValidatorArity { .. }))
    ))
}

#[test]
fn list_illegal_inhabitants() {
    let source_code = r#"
        fn main() {
          [identity]
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ))
}

#[test]
fn tuple_illegal_inhabitants() {
    let source_code = r#"
        fn main() {
          (identity, always)
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ))
}

#[test]
fn illegal_inhabitants_nested() {
    let source_code = r#"
        fn main() {
          [(identity, always)]
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ))
}

#[test]
fn illegal_function_comparison() {
    let source_code = r#"
    fn not(x: Bool) -> Bool {
      todo
    }

    fn foo() -> Bool {
      not == not
    }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalComparison { .. }))
    ))
}

#[test]
fn primitive_value_equality_is_allowed() {
    let source_code = r#"
        fn compare(left: Value, right: Value) -> Bool {
          left == right
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn primitive_value_inequality_is_allowed() {
    let source_code = r#"
        fn compare(left: Value, right: Value) -> Bool {
          left != right
        }
    "#;

    assert!(dbg!(check(parse(source_code))).is_ok())
}

#[test]
fn implicit_right_upcasting_of_value_is_allowed() {
    let source_code = r#"
        fn compare(left: Data, right: Value) -> Bool {
          left == right
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn value_literal_accepts_named_policies_assets_and_quantities() {
    let source_code = r#"
        fn make_value(
          policy: ByteArray,
          asset: ByteArray,
          quantity: Int,
        ) -> Value {
          {
            lovelace: quantity,
            policy: {
              asset: quantity,
              "static": 1,
            },
            #"11111111111111111111111111111111111111111111111111111111": {
              asset: 2,
              "static": quantity,
            },
          }
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn named_value_field_errors_use_the_identifier_span() {
    let source_code = r#"fn make_value(asset: ByteArray) -> Value {
  { missing_policy: { asset: 1 } }
}
    "#;
    let expected_start = source_code.find("missing_policy").unwrap();
    let result = check(parse(source_code));

    assert!(matches!(
        result,
        Err((_, Error::UnknownVariable { name, location, .. }))
            if name == "missing_policy"
                && location == Span::create(expected_start, "missing_policy".len())
    ));
}

#[test]
fn implicit_left_upcasting_of_value_is_allowed_symmetric() {
    let source_code = r#"
        fn compare(left: Value, right: Data) -> Bool {
          left == right
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn primitive_value_coexists_with_qualified_module_value() {
    let dependency = r#"
        pub opaque type Value {
          inner: Int
        }

        pub const zero = Value { inner: 0 }
    "#;
    let source_code = r#"
        use cardano/assets

        fn native_identity(value: Value) -> Value {
          value
        }

        fn assets_identity(value: assets.Value) -> assets.Value {
          value
        }

        fn coexist() -> assets.Value {
          expect _ = native_identity({})
          assets_identity(assets.zero)
        }
    "#;

    let result = check_with_deps(
        parse(source_code),
        vec![parse_as(dependency, "cardano/assets")],
    );

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn unqualified_cardano_assets_value_shadows_primitive_value() {
    let dependency = r#"
        pub opaque type Value {
          inner: Int
        }
    "#;
    let source_code = r#"
        use cardano/assets.{Value}

        fn imported_identity(value: Value) -> Value {
          value
        }

        fn mismatch() -> Value {
          imported_identity({})
        }
    "#;

    assert!(matches!(
        check_with_deps(
            parse(source_code),
            vec![parse_as(dependency, "cardano/assets")],
        ),
        Err((_, Error::CouldNotUnify { .. }))
    ));
}

#[test]
fn module_value_does_not_unify_with_primitive_value() {
    let dependency = r#"
        pub opaque type Value {
          inner: Int
        }

        pub const zero = Value { inner: 0 }
    "#;
    let source_code = r#"
        use cardano/assets.{zero}

        fn native_identity(value: Value) -> Value {
          value
        }

        fn mismatch() -> Value {
          native_identity(zero)
        }
    "#;

    assert!(matches!(
        check_with_deps(
            parse(source_code),
            vec![parse_as(dependency, "cardano/assets")],
        ),
        Err((_, Error::CouldNotUnify { .. }))
    ));
}

#[test]
fn generic_equality_remains_legal_for_serialisable_types() {
    let source_code = r#"
        fn same(left, right) -> Bool {
          left == right
        }

        fn compare() -> Bool {
          same(1, 2)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn generic_equality_accepts_value_specialisation() {
    let source_code = r#"
        fn same(left, right) -> Bool {
          left == right
        }

        fn compare(left: Value, right: Value) -> Bool {
          same(left, right)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn imported_generic_equality_accepts_value_specialisation() {
    let dependency = r#"
        pub fn same(left, right) -> Bool {
          left == right
        }
    "#;

    let source_code = r#"
        use equality.{same}

        fn compare(left: Value, right: Value) -> Bool {
          same(left, right)
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "equality")],).is_ok());
}

#[test]
fn prelude_generic_equality_accepts_value_specialisation() {
    let source_code = r#"
        fn same(left: Option<a>, right: Option<a>) -> Bool {
          left == right
        }

        fn compare(left: Value, right: Value) -> Bool {
          same(Some(left), Some(right))
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn piped_generic_equality_accepts_value_specialisation() {
    let source_code = r#"
        fn same(value) -> Bool {
          value == value
        }

        fn compare(value: Value) -> Bool {
          value |> same
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn pattern_unification_preserves_generic_equality_constraint() {
    let source_code = r#"
        type Box<a> {
          Box(a)
        }

        fn compare(value: Box<Value>) -> Bool {
          fn(x) {
            let result = x == x
            expect Box(_) = x
            result
          }(value)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn equality_rejects_value_nested_in_a_list() {
    let source_code = r#"
        fn compare(left: List<Value>, right: List<Value>) -> Bool {
          left == right
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn equality_rejects_value_nested_in_an_option() {
    let source_code = r#"
        fn compare(left: Option<Value>, right: Option<Value>) -> Bool {
          left == right
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn equality_accepts_value_nested_through_generic_wrappers() {
    let dependency = r#"
        pub opaque type Imported<a> {
          Imported(a)
        }
    "#;
    let source_code = r#"
        use wrapper.{Imported}

        type Local<a> {
          Local(Option<a>)
        }

        fn compare(
          left: Local<Imported<Value>>,
          right: Local<Imported<Value>>,
        ) -> Bool {
          left == right
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "wrapper")]).is_ok());
}

#[test]
fn equality_accepts_value_nested_in_a_tuple() {
    let source_code = r#"
        fn compare(left: (Value, Int), right: (Value, Int)) -> Bool {
          left == right
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn equality_accepts_value_nested_in_a_pair() {
    let source_code = r#"
        fn compare(left: Pair<Value, Int>, right: Pair<Value, Int>) -> Bool {
          left == right
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn equality_allows_value_in_a_phantom_type_argument() {
    let source_code = r#"
        type Phantom<a> {
          Phantom(Int)
        }

        fn compare(left: Phantom<Value>, right: Phantom<Value>) -> Bool {
          left == right
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn equality_allows_value_in_a_recursive_phantom_type_argument() {
    let source_code = r#"
        type RecursivePhantom<a> {
          End
          Next(Int, RecursivePhantom<a>)
        }

        fn compare(
          left: RecursivePhantom<Value>,
          right: RecursivePhantom<Value>,
        ) -> Bool {
          left == right
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn recursive_type_revisit_checks_type_arguments() {
    let source_code = r#"
        type Recursive<a> {
          End
          Next(Recursive<a>)
        }

        fn compare(
          left: Recursive<MillerLoopResult>,
          right: Recursive<MillerLoopResult>,
        ) -> Bool {
          left == right
        }
    "#;

    let result = check(parse(source_code));

    assert!(
        matches!(result, Err((_, Error::IllegalTypeInData { .. }))),
        "{result:#?}"
    )
}

#[test]
fn equality_accepts_value_in_a_generic_record_field() {
    let source_code = r#"
        type Envelope<a> {
          Envelope(a)
        }

        fn compare(left: Envelope<Value>, right: Envelope<Value>) -> Bool {
          left == right
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn equality_accepts_value_erased_by_an_opaque_wrapper() {
    let source_code = r#"
        opaque type Wrapped {
          Wrapped(Value)
        }

        fn compare(left: Wrapped, right: Wrapped) -> Bool {
          left == right
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn equality_accepts_value_erased_by_an_imported_opaque_wrapper() {
    let dependency = r#"
        pub opaque type Wrapped {
          Wrapped(Value)
        }
    "#;

    let source_code = r#"
        use wrapped.{Wrapped}

        fn compare(left: Wrapped, right: Wrapped) -> Bool {
          left == right
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "wrapped")]).is_ok());
}

#[test]
fn equality_accepts_value_hidden_behind_an_imported_private_wrapper() {
    let dependency = r#"
        type Hidden {
          Hidden(Value)
        }

        pub opaque type Public {
          Public(Hidden)
        }

        pub fn make(value: Value) -> Public {
          Public(Hidden(value))
        }
    "#;

    let source_code = r#"
        use dep.{Public, make}

        fn compare(left: Public, right: Public) -> Bool {
          left == right
        }

        test trigger() {
          let value = { #"00000000000000000000000000000000000000000000000000000000": { "foo": 14 } }
          compare(make(value), make(value))
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "dep")]).is_ok());
}

#[test]
fn equality_accepts_value_hidden_behind_a_generic_imported_private_wrapper() {
    let dependency = r#"
        type Hidden<a> {
          Hidden(a, Value)
        }

        pub opaque type Public<a> {
          Public(List<Hidden<a>>)
        }

        pub fn make(value: a) -> Public<a> {
          Public([Hidden(value, {})])
        }
    "#;

    let source_code = r#"
        use dep.{Public, make}

        fn compare(left: Public<Int>, right: Public<Int>) -> Bool {
          left == right
        }

        test trigger() {
          compare(make(1), make(1))
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "dep")]).is_ok());
}

#[test]
fn equality_allows_private_value_type_in_a_phantom_argument_of_a_public_wrapper() {
    let dependency = r#"
        type Hidden {
          Hidden(Value)
        }

        pub opaque type Phantom<a> {
          Phantom(Int)
        }

        pub opaque type Public {
          Public(Phantom<Hidden>)
        }

        pub fn make() -> Public {
          Public(Phantom(0))
        }
    "#;

    let source_code = r#"
        use dep.{Public, make}

        fn compare(left: Public, right: Public) -> Bool {
          left == right
        }

        test trigger() {
          compare(make(), make())
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "dep")]).is_ok())
}

#[test]
fn equality_accepts_private_value_type_stored_in_a_foreign_generic_wrapper() {
    let boxes = r#"
        pub opaque type Box<a> {
          Box(a)
        }

        pub fn box(inner: a) -> Box<a> {
          Box(inner)
        }
    "#;

    let dependency = r#"
        use boxes.{Box, box}

        type Hidden {
          Hidden(Value)
        }

        pub opaque type Public {
          Public(Box<Hidden>)
        }

        pub fn make(value: Value) -> Public {
          Public(box(Hidden(value)))
        }
    "#;

    let source_code = r#"
        use dep.{Public, make}

        fn compare(left: Public, right: Public) -> Bool {
          left == right
        }

        test trigger() {
          let value = {}
          compare(make(value), make(value))
        }
    "#;

    assert!(
        check_with_deps(
            parse(source_code),
            vec![parse_as(boxes, "boxes"), parse_as(dependency, "dep")],
        )
        .is_ok()
    );
}

#[test]
fn equality_allows_value_phantom_behind_an_imported_private_wrapper() {
    let dependency = r#"
        type Hidden<a> {
          Hidden(Int)
        }

        pub opaque type Public<a> {
          Public(Hidden<a>)
        }

        pub fn make() -> Public<a> {
          Public(Hidden(0))
        }
    "#;

    let source_code = r#"
        use dep.{Public, make}

        fn compare(left: Public<Value>, right: Public<Value>) -> Bool {
          left == right
        }

        test trigger() {
          compare(make(), make())
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "dep")]).is_ok())
}

#[test]
fn equality_accepts_value_stored_behind_an_imported_private_generic_wrapper() {
    let dependency = r#"
        type Hidden<a> {
          Hidden(a)
        }

        pub opaque type Public<a> {
          Public(Hidden<a>)
        }

        pub fn make(value: a) -> Public<a> {
          Public(Hidden(value))
        }
    "#;

    let source_code = r#"
        use dep.{Public, make}

        fn compare(left: Public<Value>, right: Public<Value>) -> Bool {
          left == right
        }

        test trigger() {
          let value = {}
          compare(make(value), make(value))
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "dep")],).is_ok());
}

#[test]
fn equality_allows_value_in_an_imported_opaque_phantom_argument() {
    let dependency = r#"
        pub opaque type Phantom<a> {
          Phantom(Int)
        }
    "#;

    let source_code = r#"
        use phantom.{Phantom}

        fn compare(left: Phantom<Value>, right: Phantom<Value>) -> Bool {
          left == right
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "phantom")]).is_ok())
}

#[test]
fn illegal_inhabitants_returned() {
    let source_code = r#"
        type Fuzzer<a> = fn(PRNG) -> (a, PRNG)

        fn constant(a: a) -> Fuzzer<a> {
          fn (prng) {
            (a, prng)
          }
        }

        fn main() -> Fuzzer<Fuzzer<Int>> {
          constant(constant(42))
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ))
}

#[test]
fn illegal_generic_instantiation() {
    let source_code = r#"
        type Rec<t> {
             get_t: t,
        }


        fn use_dict(dict: Rec<fn(Bool) -> Bool>, b: Bool) -> Bool {
           let f = dict.get_t
           f(b)
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ))
}

#[test]
fn not_illegal_top_level_unserialisable() {
    let source_code = r#"
        fn foo() -> MillerLoopResult {
          todo
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn illegal_unserialisable_in_generic_fn() {
    let source_code = r#"
        type Foo<a> {
          foo: a
        }

        fn main() -> Foo<fn(Int) -> Bool> {
          todo
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ))
}

#[test]
fn illegal_unserialisable_in_generic_miller_loop() {
    let source_code = r#"
        type Foo<a> {
          foo: a
        }

        fn main() -> Foo<MillerLoopResult> {
          todo
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ))
}

#[test]
fn mark_constructors_as_used_via_field_access() {
    let source_code = r#"
      type Datum {
        D0(D0Params)
        D1(D1Params)
      }

      type D0Params {
        foo: Int,
      }

      type D1Params {
        bar: Int,
      }

      fn spend(d: Datum, _r, _c) {
        when d is {
          D0(params) -> params.foo == 1
          D1(_params) -> False
        }
      }
    "#;

    let (warnings, _) = check(parse(source_code)).unwrap();

    assert_eq!(warnings.len(), 2)
}

#[test]
fn expect_multi_patterns() {
    let source_code = r#"
      fn fold(list: List<a>, initial: b, apply: fn(a, b) -> b) {
        when list is {
          [] -> initial

          [x, ..xs] -> fold(xs, apply(x, initial), apply)
        }
      }

      pub fn foo() {
        expect Some(x), acc  <- fold([Some(1), None], 0)

        x + acc
      }
    "#;

    let (warnings, _) = check(parse(source_code)).unwrap();

    assert_eq!(warnings.len(), 0)
}

#[test]
fn validator_correct_form() {
    let source_code = r#"
      validator foo {
        spend(d: Option<Data>, r, oref, c) {
          True
        }
      }
    "#;

    assert!(check_validator(parse(source_code)).is_ok())
}

#[test]
fn validator_in_lib_warning() {
    let source_code = r#"
      validator foo {
        spend(c) {
          True
        }
      }
    "#;

    let (warnings, _) = check(parse(source_code)).unwrap();

    assert!(matches!(
        warnings[0],
        Warning::ValidatorInLibraryModule { .. }
    ))
}

#[test]
fn multi_validator() {
    let source_code = r#"
      validator foo(foo: ByteArray, bar: Int) {
        spend(_d: Option<Data>, _r, _oref, _c) {
          foo == #"aabb"
        }

        mint(_r, _p, _c) {
          bar == 0
        }
      }
    "#;

    let (warnings, _) = check_validator(parse(source_code)).unwrap();

    assert_eq!(warnings.len(), 0)
}

#[test]
fn multi_validator_warning() {
    let source_code = r#"
      validator foo(foo: ByteArray, bar: Int) {
        spend(_d: Option<Data>, _r, _oref, _c) {
          foo == #"aabb"
        }

        mint(_r, _p, _c) {
          True
        }
      }
    "#;

    let (warnings, _) = check_validator(parse(source_code)).unwrap();

    assert!(matches!(
        warnings[0],
        Warning::UnusedVariable { ref name, .. } if name == "bar"
    ))
}

#[test]
fn exhaustiveness_simple() {
    let source_code = r#"
        type Foo {
          Bar
          Baz
        }

        fn foo() {
          let thing = Bar
          when thing is {
            Bar -> True
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::NotExhaustivePatternMatch {
                unmatched,
                ..
            }
        )) if unmatched[0] == "Baz"
    ))
}

#[test]
fn validator_args_no_annotation() {
    let source_code = r#"
      validator hello(d) {
        spend(a: Option<Data>, b, oref, c) {
          True
        }
      }
    "#;

    let (_, module) = check_validator(parse(source_code)).unwrap();

    module.definitions().for_each(|def| {
        let Definition::Validator(validator) = def else {
            unreachable!()
        };

        validator.params.iter().for_each(|param| {
            assert!(param.tipo.is_data());
        });

        validator.handlers[0]
            .arguments
            .iter()
            .skip(1)
            .for_each(|arg| {
                assert!(arg.tipo.is_data());
            })
    })
}

#[test]
fn exhaustiveness_missing_empty_list() {
    let source_code = r#"
        fn foo() {
          let thing = [1, 2]
          when thing is {
            [a, ..] -> True
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::NotExhaustivePatternMatch {
                unmatched,
                ..
            }
        )) if unmatched[0] == "[]"
    ))
}

#[test]
fn exhaustiveness_missing_list_wildcards() {
    let source_code = r#"
        fn foo() {
          let thing = [1, 2]
          when thing is {
            [] -> True
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::NotExhaustivePatternMatch {
                unmatched,
                ..
            }
        )) if unmatched[0] == "[_, ..]"
    ))
}

#[test]
fn exhaustiveness_missing_list_wildcards_2() {
    let source_code = r#"
        fn foo() {
          let thing = [1, 2]
          when thing is {
            [] -> True
            [a] -> True
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::NotExhaustivePatternMatch {
                unmatched,
                ..
            }
        )) if unmatched[0] == "[_, _, ..]"
    ))
}

#[test]
fn exhaustiveness_int() {
    let source_code = r#"
        fn foo() {
          let thing = 1
          when thing is {
            1 -> True
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::NotExhaustivePatternMatch {
                unmatched,
                ..
            }
        )) if unmatched[0] == "_"
    ))
}

#[test]
fn exhaustiveness_int_redundant() {
    let source_code = r#"
        fn foo() {
          let thing = 1
          when thing is {
            1 -> True
            1 -> True
            _ -> True
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::RedundantMatchClause {
                original: Some(_),
                ..
            }
        ))
    ))
}

#[test]
fn exhaustiveness_let_binding() {
    let source_code = r#"
        fn foo() {
          let Some(x) = None
          True
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::NotExhaustivePatternMatch {
                is_let,
                unmatched,
                ..
            }
        )) if unmatched[0] == "None" && is_let
    ))
}

#[test]
fn exhaustiveness_expect() {
    let source_code = r#"
        fn foo() {
          expect Some(x) = None
          True
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn exhaustiveness_expect_no_warning() {
    let source_code = r#"
        pub type A {
          int: Int,
          b: B,
        }

        pub type B {
          B0(Int)
          B1(Int)
        }

        pub fn bad_let(x: A, _: A) {
          expect A { b: B0(int), .. } = x
          int > 0
        }
    "#;

    let (warnings, _) = check(parse(source_code)).unwrap();

    assert_eq!(warnings.len(), 0)
}

#[test]
fn exhaustiveness_expect_warning() {
    let source_code = r#"
        pub type A {
          int: Int,
          b: Int,
        }

        pub fn thing(x: A, _: A) {
          expect A { b, .. } = x
          b > 0
        }
    "#;

    let (warnings, _) = check(parse(source_code)).unwrap();

    assert!(matches!(
        warnings[0],
        Warning::SingleConstructorExpect { .. }
    ))
}

#[test]
fn exhaustiveness_missing_constr_with_args() {
    let source_code = r#"
        type Foo {
          Bar
          Why(Int)
          Baz { other: Int }
        }

        fn foo() {
          let thing = Bar
          when thing is {
            Bar -> True
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::NotExhaustivePatternMatch {
                unmatched,
                ..
            }
        )) if unmatched[0] == "Why(_)" && unmatched[1] == "Baz { other }"
    ))
}

#[test]
fn exhaustiveness_redundant_pattern() {
    let source_code = r#"
        type Foo {
          A
          B
        }

        fn foo(a: Foo) {
          when a is {
            A -> todo
            B -> todo
            _ -> todo
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::RedundantMatchClause { original: None, .. }))
    ))
}

#[test]
fn exhaustiveness_redundant_pattern_2() {
    let source_code = r#"
        type Foo {
          A
          B
        }

        fn foo(a: Foo) {
          when a is {
            A -> todo
            B -> todo
            A -> todo
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::RedundantMatchClause {
                original: Some(_),
                ..
            }
        ))
    ))
}

#[test]
fn exhaustiveness_complex() {
    let source_code = r#"
        type Foo {
          Bar
          Why(Int)
          Baz { other: Int }
        }

        type Hello {
          Yes
          No { idk: Int, thing: Foo }
        }

        fn foo() {
          let thing = ((Yes, 1), (Yes, [1, 2]))
          when thing is {
            ((Yes, _), (Yes, [])) -> True
            ((Yes, _), (No { .. }, _)) -> True
            ((No { .. }, _), (No { .. }, _)) -> True
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::NotExhaustivePatternMatch {
                unmatched,
                ..
            }
        )) if unmatched[0] == "((Yes, _), (Yes, [_, ..]))" && unmatched[1] == "((No { idk, thing }, _), (Yes, _))"
    ))
}

#[test]
fn exhaustiveness_tuple() {
    let source_code = r#"
        fn foo() {
            when (14, True) is {
                (14, True) -> Void
            }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::NotExhaustivePatternMatch {
                unmatched,
                ..
            }
        )) if unmatched[0] == "(_, _)"
    ))
}

#[test]
fn exhaustiveness_nested_list_and_tuples() {
    fn assert_step(step: &str, expected: &str) {
        let result = check(parse(step));
        assert!(matches!(
            result,
            Err((
                _,
                Error::NotExhaustivePatternMatch {
                    unmatched,
                    ..
                }
            )) if unmatched[0] == expected
        ));
    }

    assert_step(
        r#"
        fn foo() {
            let xs : List<(List<(Int, Bool)>, Int)> = [([(14, True)], 42)]
            when xs is {
                [                      ] -> Void
                [([(14, True)], 42), ..] -> Void
            }
        }
        "#,
        "[([], _), ..]",
    );

    assert_step(
        r#"
        fn foo() {
            let xs : List<(List<(Int, Bool)>, Int)> = [([(14, True)], 42)]
            when xs is {
                [                     ] -> Void
                [([(_, True)], 42), ..] -> Void
                [([         ],  _), ..] -> Void
            }
        }
        "#,
        "[([(_, False), ..], _), ..]",
    );

    assert_step(
        r#"
        fn foo() {
            let xs : List<(List<(Int, Bool)>, Int)> = [([(14, True)], 42)]
            when xs is {
                [                          ] -> Void
                [([(_, True )    ], 42), ..] -> Void
                [([              ],  _), ..] -> Void
                [([(_, False), ..],  _), ..] -> Void
            }
        }
        "#,
        "[([(_, True), _, ..], _), ..]",
    );

    assert_step(
        r#"
        fn foo() {
            let xs : List<(List<(Int, Bool)>, Int)> = [([(14, True)], 42)]
            when xs is {
                [                             ] -> Void
                [([(_, True )       ], 42), ..] -> Void
                [([                 ],  _), ..] -> Void
                [([(_, False)   , ..],  _), ..] -> Void
                [([(_, True ), _, ..],  _), ..] -> Void
            }
        }
        "#,
        "[([(_, True)], _), ..]",
    );

    let source_code = r#"
        fn foo() {
            let xs : List<(List<(Int, Bool)>, Int)> = [([(14, True)], 42)]
            when xs is {
                [                             ] -> Void
                [([(_, True )       ], 42), ..] -> Void
                [([                 ],  _), ..] -> Void
                [([(_, False)   , ..],  _), ..] -> Void
                [([(_, True ), _, ..],  _), ..] -> Void
                [([(_, True )       ],  _), ..] -> Void
            }
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn expect_sugar_correct_type() {
    let source_code = r#"
        fn foo() {
          expect 1 == 1
          2
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn expect_sugar_incorrect_type() {
    let source_code = r#"
        fn foo() {
          expect 1
          2
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn logical_op_chain_expressions_should_be_bool() {
    let source_code = r#"
        fn foo() {
          and {
            1 == 1,
            False,
            or {
              2 == 3,
              1
            }
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn anonymous_function_scoping() {
    let source_code = r#"
        fn reduce(list, f, i) {
          todo
        }

        pub fn foo() {
          let sum =
            reduce(
              [1, 2, 3],
              fn(acc: Int, n: Int) { acc + n },
              0,
            )

          sum + acc
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::UnknownVariable { name, .. })) if name == "acc"
    ))
}

#[test]
fn anonymous_function_dupicate_args() {
    let source_code = r#"
        fn reduce(list, f, i) {
          todo
        }

        pub fn foo() {
          let sum =
            reduce(
              [1, 2, 3],
              fn(acc: Int, acc: Int) { acc + acc },
              0,
            )

          sum
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::DuplicateArgument { label, .. })) if label == "acc"
    ))
}

#[test]
fn record_update_duplicate_labels() {
    let source_code = r#"
        pub type User {
          User {
            name: ByteArray,
            age: Int,
          }
        }

        pub fn foo() {
          let user = User { name: "ada", age: 10 }
          User { ..user, age: 20, age: 30 }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::DuplicateRecordUpdateArgument { label, .. })) if label == "age"
    ))
}

#[test]
fn assignement_last_expr_when() {
    let source_code = r#"
        pub fn foo() {
          let bar = None

          when bar is {
            Some(_) -> {
              let wow = 1
            }
            None -> {
              2
            }
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::LastExpressionIsAssignment { .. }))
    ))
}

#[test]
fn assignement_last_expr_if_first_branch() {
    let source_code = r#"
        pub fn foo() {
          if True {
            let thing = 1
          } else {
            1
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::LastExpressionIsAssignment { .. }))
    ))
}

#[test]
fn assignement_last_expr_if_branches() {
    let source_code = r#"
        pub fn foo() {
          if True {
            2
          } else if False {
            let thing = 1
          } else {
            1
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::LastExpressionIsAssignment { .. }))
    ))
}

#[test]
fn assignement_last_expr_if_final_else() {
    let source_code = r#"
        pub fn foo() {
          if True {
            1
          } else {
            let thing = 1
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::LastExpressionIsAssignment { .. }))
    ))
}

#[test]
fn assignment_last_expr_logical_chain() {
    let source_code = r#"
        pub fn foo() -> Bool {
            and {
                expect 1 + 1 == 2,
                True,
                2 > 0,
                or {
                    expect True,
                    False,
                }
            }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::LastExpressionIsAssignment { .. }))
    ))
}

#[test]
fn if_scoping() {
    let source_code = r#"
        pub fn foo(c) {
          if c {
            let bar = 1
            bar
          } else if !c {
            bar
          } else {
            bar
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::UnknownVariable { .. }))
    ))
}

#[test]
fn list_pattern_1() {
    let source_code = r#"
        test foo() {
          let xs = [1, 2, 3]
          let [x] = xs
          x == 1
        }
    "#;
    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::NotExhaustivePatternMatch { .. }))
    ))
}

#[test]
fn list_pattern_2() {
    let source_code = r#"
        test foo() {
          let xs = [1, 2, 3]
          let x = when xs is {
            [x] -> x
          }
          x == 1
        }
    "#;
    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::NotExhaustivePatternMatch { .. }))
    ))
}

#[test]
fn list_pattern_3() {
    let source_code = r#"
        test foo() {
          let xs = [1, 2, 3]
          let x = when xs is {
            [x] -> x
            [x, ..] -> x
          }
          x == 1
        }
    "#;
    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::NotExhaustivePatternMatch { .. }))
    ))
}

#[test]
fn list_pattern_4() {
    let source_code = r#"
        test foo() {
          let xs = [1, 2, 3]
          let x = when xs is {
            [] -> 1
            [_, ..] -> 1
          }
          x == 1
        }
    "#;
    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn list_pattern_5() {
    let source_code = r#"
        test foo() {
          let xs = [1, 2, 3]
          let x = when xs is {
            [x, ..] -> 1
            _ -> 1
          }
          x == 1
        }
    "#;
    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn spread_with_positional_constr_args() {
    let source_code = r#"
        type Redeemer {
          First(Int)
          Second
        }

        fn foo(redeemer: Redeemer) {
          when redeemer is {
            First(..) -> True
            Second -> True
          }
        }
    "#;
    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn unnecessary_spread_with_positional_constr_args() {
    let source_code = r#"
        type Redeemer {
          First(Int)
          Second
        }

        fn foo(redeemer: Redeemer) {
          when redeemer is {
            First(x, ..) -> True
            Second -> True
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::UnnecessarySpreadOperator { .. }))
    ))
}

#[test]
fn trace_strings() {
    let source_code = r#"
        fn bar() {
            @"BAR"
        }

        test foo() {
            let msg1 = @"FOO"
            trace(@"INLINE")
            trace(msg1)
            trace(bar())
            True
        }
    "#;
    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn trace_non_strings() {
    let source_code = r#"
        test foo() {
            trace(14 + 42)
            True
        }
    "#;
    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn trace_string_label_compact() {
    let source_code = r#"
        test foo() {
            trace @"foo": [1,2,3]
            True
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn trace_non_string_label_compact() {
    let source_code = r#"
        test foo() {
            trace(14 + 42)
            True
        }
    "#;

    assert!(matches!(
        &check_with_verbosity(parse(source_code), TraceLevel::Compact),
        Ok((warnings, _)) if warnings == &[Warning::CompactTraceLabelIsNotstring { location: Span::create(40, 7) }],
    ))
}

#[test]
fn trace_if_false_ok() {
    let source_code = r#"
        fn or_func(a: Bool, b: Bool) {
           (a || b)?
        }

        test foo() {
            or_func(True, False)?
        }

        test bar() {
            let must_be_signed = True
            must_be_signed?
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn backpassing_basic() {
    let source_code = r#"
        fn and_then(opt: Option<a>, then: fn(a) -> Option<b>) -> Option<b> {
          when opt is {
            None -> None
            Some(a) -> then(a)
          }
        }

        fn backpassing(opt_i: Option<Int>, opt_j: Option<Int>) -> Option<Int> {
          let i <- and_then(opt_i)
          let j <- and_then(opt_j)
          Some(i + j)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn backpassing_expect_simple() {
    let source_code = r#"
        fn and_then(opt: Option<a>, then: fn(a) -> Option<b>) -> Option<b> {
          when opt is {
            None -> None
            Some(a) -> then(a)
          }
        }

        fn backpassing(opt_i: Option<Int>, opt_j: Option<Int>) -> Option<Int> {
          expect 42 <- and_then(opt_i)
          let j <- and_then(opt_j)
          Some(j + 42)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn backpassing_expect_nested() {
    let source_code = r#"
        fn and_then(opt: Option<a>, then: fn(Option<a>) -> Option<b>) -> Option<b> {
          when opt is {
            None -> None
            Some(a) -> then(Some(a))
          }
        }

        fn backpassing(opt_i: Option<Int>, opt_j: Option<Int>) -> Option<Int> {
          expect Some(i) <- and_then(opt_i)
          expect Some(j) <- and_then(opt_j)
          Some(i + j)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn backpassing_interleaved_capture() {
    let source_code = r#"
        fn and_then(opt: Option<a>, then: fn(a) -> Option<b>) -> Option<b> {
          when opt is {
            None -> None
            Some(a) -> then(a)
          }
        }

        fn backpassing(opt_i: Option<Int>, opt_j: Option<Int>) -> Option<Int> {
          let f = and_then(opt_i, _)
          let i <- f
          let g = and_then(opt_j, _)
          let j <- g
          Some(i + j)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn backpassing_patterns() {
    let source_code = r#"
        fn and_then(opt: Option<a>, then: fn(a) -> Option<b>) -> Option<b> {
          when opt is {
            None -> None
            Some(a) -> then(a)
          }
        }

        type Foo {
          foo: Int,
        }

        fn backpassing(opt_i: Option<Foo>, opt_j: Option<Foo>) -> Option<Int> {
          let Foo { foo: i } <- and_then(opt_i)
          let Foo { foo: j } <- and_then(opt_j)
          Some(i + j)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn backpassing_not_a_function() {
    let source_code = r#"
        fn and_then(opt: Option<a>, then: fn(a) -> Option<b>) -> Option<b> {
          when opt is {
            None -> None
            Some(a) -> then(a)
          }
        }

        fn backpassing(opt_i: Option<Int>, opt_j: Option<Int>) -> Option<Int> {
          let i <- opt_i
          let j <- and_then(opt_j)
          Some(i + j)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::NotFn { .. }))
    ))
}

#[test]
fn backpassing_non_exhaustive_pattern() {
    let source_code = r#"
        fn and_then(opt: Option<a>, then: fn(a) -> Option<b>) -> Option<b> {
          when opt is {
            None -> None
            Some(a) -> then(a)
          }
        }

        fn backpassing(opt_i: Option<Int>, opt_j: Option<Int>) -> Option<Int> {
          let 42 <- and_then(opt_i)
          let j <- and_then(opt_j)
          Some(i + j)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::NotExhaustivePatternMatch { .. }))
    ))
}

#[test]
fn backpassing_unsaturated_fn() {
    let source_code = r#"
        fn and_then(opt: Option<a>, then: fn(a) -> Option<b>) -> Option<b> {
          when opt is {
            None -> None
            Some(a) -> then(a)
          }
        }

        fn backpassing(opt_i: Option<Int>, opt_j: Option<Int>) -> Option<Int> {
          let i <- and_then
          let j <- and_then(opt_j)
          Some(i + j)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IncorrectFieldsArity { .. }))
    ))
}

#[test]
fn backpassing_expect_type_mismatch() {
    let source_code = r#"
        fn and_then(opt: Option<a>, then: fn(a) -> Option<b>) -> Option<b> {
          when opt is {
            None -> None
            Some(a) -> then(a)
          }
        }

        fn backpassing(opt_i: Option<Int>, opt_j: Option<Int>) -> Option<Int> {
          expect Some(i) <- and_then(opt_i)
          let j <- and_then(opt_j)
          Some(i + j)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn backpassing_multi_args() {
    let source_code = r#"
        fn fold(list: List<a>, init: b, then: fn(a, b) -> b) -> b {
          when list is {
            [] -> init
            [x, ..rest] -> fold(rest, then(x, init), then)
          }
        }

        fn backpassing() -> Int {
          let elem, acc <- fold([1, 2, 3], 0)

          elem + acc
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn backpassing_multi_args_expect() {
    let source_code = r#"
        pub type Bar {
          Foo(Int)
          Wow(Int)
        }

        fn fold(list: List<a>, init: b, then: fn(a, b) -> b) -> b {
          when list is {
            [] -> init
            [x, ..rest] -> fold(rest, then(x, init), then)
          }
        }

        pub fn backpassing() -> Bar {
          expect Foo(elem), Wow(acc) <- fold([Foo(1), Foo(2), Foo(3)], Wow(0))

          Wow(elem + acc)
        }
    "#;

    assert!(matches!(check(parse(source_code)), Ok((warnings, _)) if warnings.is_empty()))
}

#[test]
fn backpassing_multi_args_using_equals() {
    let source_code = r#"
        fn fold(list: List<a>, init: b, then: fn(a, b) -> b) -> b {
          when list is {
            [] -> init
            [x, ..rest] -> fold(rest, then(x, init), then)
          }
        }

        fn backpassing() -> Int {
          let elem, acc = fold([1, 2, 3], 0, fn(elem, acc) { elem + acc })

          elem + acc
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::UnexpectedMultiPatternAssignment { .. }))
    ))
}

#[test]
fn trace_if_false_ko() {
    let source_code = r#"
        fn add(a: Int, b: Int) {
           (a + b)?
        }

        test foo() {
            add(14, 42) == 12
        }

        test bar() {
            let must_be_signed = #"FF00"
            must_be_signed? == #"FF00"
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn pipe_with_wrong_type() {
    let source_code = r#"
        test foo() {
          True |> bar
        }

        fn bar(n: Int) {
          n - 1
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::CouldNotUnify {
                situation: Some(UnifyErrorSituation::PipeTypeMismatch),
                ..
            }
        ))
    ))
}

#[test]
fn pipe_with_wrong_type_and_args() {
    let source_code = r#"
        test foo() {
          True |> bar(False)
        }

        fn bar(n: Int, l: Bool) {
          n - 1
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::CouldNotUnify {
                situation: Some(UnifyErrorSituation::PipeTypeMismatch),
                ..
            }
        ))
    ))
}

#[test]
fn pipe_with_right_type_and_wrong_args() {
    let source_code = r#"
        test foo() {
          1 |> bar(1)
        }

        fn bar(n: Int, l: Bool) {
          n - 1
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::CouldNotUnify {
                situation: None,
                ..
            }
        ))
    ))
}

#[test]
fn pipe_with_wrong_type_and_full_args() {
    let source_code = r#"
        test foo() {
          True |> bar(False)
        }

        fn bar(l: Bool) -> fn(Int) -> Int {
          fn(n: Int) {
            n - 1
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::CouldNotUnify {
                situation: Some(UnifyErrorSituation::PipeTypeMismatch),
                ..
            }
        ))
    ))
}

#[test]
fn pipe_wrong_arity_partially_applied() {
    let source_code = r#"
        fn f(_a: Int, _b: Int, _c: Int) -> Int {
            todo
        }

        test foo() {
            0 |> f(0)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IncorrectFieldsArity { given, expected, .. })) if given == 2 && expected == 3
    ))
}

#[test]
fn pipe_wrong_arity_fully_saturated() {
    let source_code = r#"
        fn f(_a: Int, _b: Int, _c: Int) -> Int {
            todo
        }

        test foo() {
            0 |> f(0, 0, 0)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::NotFn { .. }))
    ))
}

#[test]
fn pipe_wrong_arity_fully_saturated_return_fn() {
    let source_code = r#"
        fn f(_a: Int, _b: Int, _c: Int) -> fn(Int) -> Int {
            todo
        }

        test foo() {
            (0 |> f(0, 0, 0)) == 0
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn fuzzer_ok_basic() {
    let source_code = r#"
        fn int() -> Fuzzer<Int> { todo }
        test prop(n via int()) { True }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn sampler_ok_basic() {
    let source_code = r#"
        fn int() -> Sampler<Int> { todo }
        bench prop(n via int()) { True }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn fuzzer_ok_explicit() {
    let source_code = r#"
        fn int(prng: PRNG) -> Option<(PRNG, Int)> { todo }
        test prop(n via int) { Void }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn fuzzer_ok_list() {
    let source_code = r#"
        fn int() -> Fuzzer<Int> { todo }
        fn list(a: Fuzzer<a>) -> Fuzzer<List<a>> { todo }

        test prop(xs via list(int())) { True }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn fuzzer_err_unbound() {
    let source_code = r#"
        fn any() -> Fuzzer<a> { todo }
        fn list(a: Fuzzer<a>) -> Fuzzer<List<a>> { todo }

        test prop(xs via list(any())) { todo }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::GenericLeftAtBoundary { .. }))
    ))
}

#[test]
fn fuzzer_err_unify_1() {
    let source_code = r#"
        test prop(xs via Void) { todo }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::CouldNotUnify {
                situation: None,
                ..
            }
        ))
    ))
}

#[test]
fn fuzzer_err_unify_2() {
    let source_code = r#"
        fn any() -> Fuzzer<a> { todo }
        test prop(xs via any) { todo }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::CouldNotUnify {
                situation: None,
                ..
            }
        ))
    ))
}

#[test]
fn fuzzer_err_unify_3() {
    let source_code = r#"
        fn list(a: Fuzzer<a>) -> Fuzzer<List<a>> { todo }
        fn int() -> Fuzzer<Int> { todo }

        test prop(xs: Int via list(int())) { todo }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::CouldNotUnify {
                situation: Some(UnifyErrorSituation::FuzzerAnnotationMismatch),
                ..
            }
        ))
    ))
}

#[test]
fn fuzzer_unify_across_deps() {
    let dependency = r#"
        pub type Credential {
          VerificationKey(ByteArray)
          Script(ByteArray)
        }
    "#;

    let source_code = r#"
        use cardano/address.{Credential}

        fn fuzz_credential(prng: PRNG) -> Option<(PRNG, Credential)> {
            Some((prng, Credential.Script("")))
        }

        test foo(c: Credential via fuzz_credential) {
            c == Credential.Script("")
        }
    "#;

    let result = check_with_deps(
        parse(source_code),
        vec![(parse_as(dependency, "cardano/address"))],
    );

    assert!(matches!(result, Ok(..)), "{result:#?}");
}

#[test]
fn utf8_hex_literal_warning() {
    let source_code = r#"
        pub const policy_id = "f43a62fdc3965df486de8a0d32fe800963589c41b38946602a0dc535"
    "#;

    let (warnings, _) = check(parse(source_code)).unwrap();

    assert!(matches!(
        warnings[0],
        Warning::Utf8ByteArrayIsValidHexString { .. }
    ))
}

#[test]
fn discarded_let_bindings() {
    let source_code = r#"
        fn foo() {
            let result = when 42 is {
                1 -> {
                    let unused = "foo"
                    Void
                }
                _ -> {
                    Void
                }
            }

            let _ = "foo"

            result
        }
    "#;

    let (warnings, ast) = check(parse(source_code)).unwrap();

    assert!(matches!(warnings[0], Warning::UnusedVariable { ref name, .. } if name == "unused"));
    assert!(matches!(warnings[1], Warning::DiscardedLetAssignment { ref name, .. } if name == "_"));

    // Controls that unused let-bindings have been erased from the transformed AST.
    match ast.definitions.first() {
        Some(Definition::Fn(def)) => match &def.body {
            TypedExpr::Sequence { expressions, .. } => {
                assert_eq!(expressions.len(), 2);
                assert!(
                    matches!(expressions[1], TypedExpr::Var { .. }),
                    "last expression isn't return variable"
                );
                match &expressions[0] {
                    TypedExpr::Assignment { value, .. } => match **value {
                        TypedExpr::When { ref clauses, .. } => {
                            assert!(
                                matches!(clauses[0].then, TypedExpr::Sequence { ref expressions, ..} if expressions.len() == 1)
                            )
                        }
                        _ => unreachable!("first expression isn't when/is"),
                    },
                    _ => unreachable!("first expression isn't assignment"),
                }
            }
            _ => unreachable!("body isn't a Sequence"),
        },
        _ => unreachable!("ast isn't a Fn"),
    }
}

#[test]
fn unused_expect_bindings_are_not_erased() {
    let source_code = r#"
        use aiken/builtin

        type Small {
          Small(Int)
        }

        test foo() {
            let raw = builtin.list_data([builtin.constr_data(0, [])])

            expect xs: List<Small> = raw

            True
        }
    "#;

    let (warnings, ast) = check_validator(parse(source_code)).unwrap();

    assert!(
        warnings
            .iter()
            .any(|warning| matches!(warning, Warning::UnusedVariable { name, .. } if name == "xs"))
    );

    let test_def = ast
        .definitions
        .iter()
        .find_map(|def| match def {
            Definition::Test(def) => Some(def),
            _ => None,
        })
        .expect("expected a test definition");

    match &test_def.body {
        TypedExpr::Sequence { expressions, .. } => {
            assert_eq!(expressions.len(), 3);
            assert!(matches!(expressions[0], TypedExpr::Assignment { .. }));
            assert!(
                matches!(expressions[1], TypedExpr::Assignment { kind, .. } if kind.is_expect()),
                "unused expect assignment should remain in the transformed AST"
            );
        }
        _ => unreachable!("test body isn't a sequence"),
    }
}

#[test]
fn backpassing_type_annotation() {
    let source_code = r#"
        pub type Foo {
          foo: Int,
        }

        fn transition_fold4(
          inputs,
          callback,
        ) {
          when inputs is {
            [] -> {
              (Foo(1), inputs)
            }
            [input, ..remaining_inputs] -> {

              callback(input)(
                fn(foo) {
                  transition_fold4(
                    remaining_inputs,
                    callback,
                  )
                },
              )
            }
          }
        }

        pub fn backpassing(x) {
          let input: Foo <-
            transition_fold4(
              x,
            )

          fn(g){
            g(if input.foo == 1{
              1
            } else {
              2
            })
          }

        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn forbid_expect_into_opaque_type_from_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        fn bar(n: Data) {
          expect a: Thing = n

          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_incomplete_expect_into_opaque_type_from_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }
        type Bar { a0: Int, b0: Thing }

        fn bar(data: Data) {
          expect Bar { b0, .. } = data
          b0
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_complete_expect_into_record_containing_opaque_type_from_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }
        type Bar { a0: Int, b0: Thing }

        fn bar(data: Data) {
          expect Bar { a0, b0 } = data
          b0
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_constructor_containing_opaque_type_from_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }
        type Bar {
          Bar(Int, Thing)
        }

        fn bar(data: Data) {
          expect Bar(_, b0) = data
          b0
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_pair_containing_opaque_type_from_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        fn bar(data: Data) {
          expect pair: Pair<Thing, Int> = data
          pair
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_tuple_containing_opaque_type_from_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        fn bar(data: Data) {
          expect tuple: (Thing, Int) = data
          tuple
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_record_with_pair_containing_opaque_type_from_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }
        type Bar { pair: Pair<Thing, Int> }

        fn bar(data: Data) {
          expect Bar { pair } = data
          pair
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_record_with_tuple_containing_opaque_type_from_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }
        type Bar { tuple: (Thing, Int) }

        fn bar(data: Data) {
          expect Bar { tuple } = data
          tuple
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_imported_record_containing_opaque_type_from_data() {
    let dependency = r#"
        pub opaque type Thing { inner: Int }
        pub type Bar { a0: Int, b0: Thing }
    "#;

    let source_code = r#"
        use thing.{Bar}

        fn bar(data: Data) {
          expect Bar { b0, .. } = data
          b0
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))]),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_imported_record_with_pair_containing_opaque_type_from_data() {
    let dependency = r#"
        pub opaque type Thing { inner: Int }
        pub type Bar { pair: Pair<Thing, Int> }
    "#;

    let source_code = r#"
        use thing.{Bar}

        fn bar(data: Data) {
          expect Bar { pair } = data
          pair
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))]),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_imported_record_with_tuple_containing_opaque_type_from_data() {
    let dependency = r#"
        pub opaque type Thing { inner: Int }
        pub type Bar { tuple: (Thing, Int) }
    "#;

    let source_code = r#"
        use thing.{Bar}

        fn bar(data: Data) {
          expect Bar { tuple } = data
          tuple
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))]),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_nested_imported_opaque_types() {
    let dependency = r#"
        pub opaque type Thing { inner: Int }

        pub type Outer { inner: Inner }
        pub type Inner { thing: Thing }
    "#;

    let source_code = r#"
        use thing.{Outer}

        fn bar(data: Data) {
          expect Outer { inner } = data
          inner
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))]),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_nested_imported_nested_opaque_types() {
    let dependency = r#"
        pub opaque type Thing { inner: Int }

        pub type Outer { inner: Inner }
        pub type Inner { thing: Thing }
    "#;

    let source_code = r#"
        use thing.{Outer}

        pub type Baz {
            Baz1(Int)
            Baz2(Outer)
        }

        fn baz(data: Data) {
          expect Baz1(inner) = data
          inner
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))]),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_alias_of_record_containing_opaque_type_from_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }
        type Bar { a0: Int, b0: Thing }
        type BarAlias =
          Bar

        fn bar(data: Data) {
          expect a: BarAlias = data
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_imported_alias_of_record_containing_opaque_type_from_data() {
    let dependency = r#"
        pub opaque type Thing { inner: Int }
        pub type Bar = Thing
    "#;

    let source_code = r#"
        use thing.{Bar}

        fn bar(data: Data) {
          expect Bar { inner }: Bar = data
          inner
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))]),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_nested_opaque_types() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        type Outer { inner: Inner }
        type Inner { thing: Thing }

        fn bar(data: Data) {
          expect Outer { inner } = data
          inner
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_partial_down_casting() {
    let source_code = r#"
        type Foo {
          x: Int
        }

        fn bar(n: List<Foo>) {
          expect a: List<Data> = n
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn forbid_partial_up_casting() {
    let source_code = r#"
        type Foo {
          x: Int
        }

        fn bar(n: List<Data>) {
          expect a: List<Foo> = n
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn allow_expect_into_type_from_data() {
    let source_code = r#"
        fn bar(n: Data) {
          expect a: Option<Int> = n
          a
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn forbid_casting_into_type_from_data() {
    let source_code = r#"
        fn bar(n: Data) {
          let a: Option<Int> = n
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn forbid_casting_into_var_from_data_with_ann() {
    let source_code = r#"
        fn bar(n: Data) {
          let a: Option<Int> = n
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn allow_let_rebinding() {
    let source_code = r#"
        fn bar(n: Data) {
          let a = n
          a
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn expect_rebinding_requires_annotation() {
    let source_code = r#"
        fn bar(n: Data) -> Option<Int> {
          expect a = n
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CastDataNoAnn { .. }))
    ))
}

#[test]
fn forbid_casting_into_var_from_data_with_ann_indirect() {
    let source_code = r#"
        fn bar(n: Data) -> Option<Int> {
          let a = n
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn forbid_casting_into_pattern_from_data() {
    let source_code = r#"
        fn bar(n: Data) {
          let Some(a) = n
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn allow_expect_into_monomorphic_type_from_data_with_pattern() {
    let source_code = r#"
        fn bar(n: Data) {
          expect Some(a): Option<Int> = n
          a
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn forbid_expect_into_generic_type_from_data_with_pattern() {
    let source_code = r#"
        fn bar(n: Data) {
          expect Some(a) = n
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CastDataNoAnn { .. }))
    ))
}

#[test]
fn allow_generic_expect_without_typecast() {
    let source_code = r#"
        pub fn unwrap(opt: Option<a>) -> a {
          expect Some(a) = opt
          a
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn allow_expect_into_custom_type_from_data_no_annotation() {
    let source_code = r#"
        type OrderDatum {
            requested_handle: ByteArray,
            amount: Int,
            other: Bool,
        }

        fn foo(datum: Data) {
            expect OrderDatum { requested_handle, .. } = datum
            requested_handle
        }

    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn forbid_expect_from_arbitrary_type() {
    let source_code = r#"
        type Foo {
          x: Int
        }

        type Bar {
          y: Int
        }

        fn bar(f: Foo) {
          expect b: Bar = f
          Void
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn allow_expect_into_opaque_type_constructor_without_typecasting_in_module() {
    let source_code = r#"
        opaque type Thing {
          Foo(Int)
          Bar(Int)
        }

        fn bar(thing: Thing) {
          expect Foo(a) = thing
          a
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn use_imported_type_as_namespace_for_patterns() {
    let dependency = r#"
        pub type Credential {
          VerificationKey(ByteArray)
          Script(ByteArray)
        }
    "#;

    let source_code = r#"
        use cardano/address.{Credential, Script, VerificationKey}

        pub fn compare(left: Credential, right: Credential) -> Ordering {
          when left is {
            Script(left) ->
              when right is {
                Script(right) -> Equal
                _ -> Less
              }
            VerificationKey(left) ->
              when right is {
                Script(_) -> Greater
                VerificationKey(right) -> Equal
              }
          }
        }
    "#;

    let result = check_with_deps(
        parse(source_code),
        vec![(parse_as(dependency, "cardano/address"))],
    );

    assert!(matches!(result, Ok(..)), "{result:#?}");
}

#[test]
fn use_type_as_namespace_for_patterns() {
    let dependency = r#"
        pub type Foo {
          A { a: Int }
          B
        }
    "#;

    let source_code = r#"
        use thing.{Foo}

        fn bar(foo: Foo) {
          when foo is {
            Foo.A { a } -> a > 10
            Foo.B -> False
          }
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))]);

    assert!(matches!(result, Ok(..)), "{result:#?}");
}

#[test]
fn use_type_as_namespace_for_patterns_marked_used() {
    let dependency = r#"
        pub type Foo {
          I(Int)
          B(Bool)
        }

        pub type Bar {
          Bar1
          Bar2
        }
    "#;

    let source_code = r#"
        use foo.{Foo, Bar}

        pub fn foo_to_string(x) -> String {
          when x is {
            Foo.I(..) -> @"I"
            Foo.B(..) -> @"B"
          }
        }
    "#;

    let (warnings, _) =
        check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]).unwrap();

    assert!(
        matches!(warnings.as_slice(), &[Warning::UnusedImportedValueOrType { ref name, .. }] if name == "Bar"),
        "{warnings:#?}"
    );
}

#[test]
fn use_nested_type_as_namespace_for_patterns() {
    let dependency = r#"
        pub type Foo {
          A { a: Int }
          B
        }
    "#;

    let source_code = r#"
        use foo.{Foo}

        fn bar(x: Foo) {
          when x is {
            foo.Foo.A { a } -> a > 10
            foo.Foo.B -> False
          }
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(matches!(result, Ok(..)), "{result:#?}");
}

#[test]
fn use_opaque_type_as_namespace_for_patterns_fails() {
    let dependency = r#"
        pub opaque type Foo {
          A { a: Int }
          B
        }
    "#;

    let source_code = r#"
        use thing.{Foo}

        fn bar(foo: Foo) {
          when foo is {
            Foo.A { a } -> a > 10
            Foo.B -> False
          }
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownTypeConstructor { name, .. })) if name == "A" && warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn use_wrong_type_as_namespace_for_patterns_fails() {
    let dependency = r#"
        pub type Foo {
          A { a: Int }
          B
        }

        pub type Bar {
          C
          D
        }
    "#;

    let source_code = r#"
        use thing.{Foo}

        fn bar(foo: Foo) {
          when foo is {
            Foo.A { .. } -> True
            Foo.D -> False
          }
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownTypeConstructor { name, .. })) if name == "D" && warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn use_type_as_namespace_for_constructors() {
    let dependency = r#"
        pub type Foo {
          I { i: Int }
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo.{Foo}

        test my_test() {
          and {
            Foo.I { i: 42 } == foo.I(42),
            foo.B(True) == Foo.B(True),
          }
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(matches!(result, Ok(..)), "{result:#?}");
}

#[test]
fn use_type_as_nested_namespace_for_constructors() {
    let dependency = r#"
        pub type Foo {
          I { i: Int }
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo

        test my_test() {
          trace foo.Foo.I { i: 42 }
          trace foo.Foo.B(False)
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(matches!(result, Ok(..)), "{result:#?}");
}

#[test]
fn use_type_as_nested_namespace_for_constructors_from_multi_level_module() {
    let dependency = r#"
        pub type Foo {
          I { i: Int }
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo/bar

        test my_test() {
          trace bar.Foo.I { i: 42 }
          trace bar.Foo.B(True)
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo/bar"))]);

    assert!(matches!(result, Ok(..)), "{result:#?}");
}

#[test]
fn use_type_as_nested_namespace_for_constructors_from_module_alias() {
    let dependency = r#"
        pub type Foo {
          I { i: Int }
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo as bar

        test my_test() {
          trace bar.Foo.I { i: 42 }
          trace bar.Foo.B(True)
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(matches!(result, Ok(..)), "{result:#?}");
}

#[test]
fn use_type_as_namespace_unknown_constructor() {
    let dependency = r#"
        pub type Foo {
          I { i: Int }
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo.{Foo}

        test my_test() {
          Foo.I(42) == Foo.A
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownTypeConstructor { name, .. })) if name == "A" && warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn use_wrong_type_as_namespace() {
    let dependency = r#"
        pub type Foo {
          I { i: Int }
          B(Bool)
        }

        pub type Bar {
          S { s: String }
          L(List<Int>)
        }
    "#;

    let source_code = r#"
        use foo.{Foo}

        test my_test() {
          trace Foo.S(@"wut")
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownTypeConstructor { name, .. })) if name == "S" && warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn use_wrong_nested_type_as_namespace() {
    let dependency = r#"
        pub type Foo {
          I { i: Int }
          B(Bool)
        }

        pub type Bar {
          S { s: String }
          L(List<Int>)
        }
    "#;

    let source_code = r#"
        use foo

        test my_test() {
          trace foo.Foo.S(@"wut")
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownTypeConstructor { name, .. })) if name == "S" && warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn use_private_type_as_nested_namespace_fails() {
    let dependency = r#"
        type Foo {
          I { i: Int }
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo

        test my_test() {
          trace foo.Foo.I { i: 42 }
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownModuleType { name, .. })) if name == "Foo" && warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn use_opaque_type_as_namespace_for_constructors_fails() {
    let dependency = r#"
        pub opaque type Foo {
          I { i: Int }
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo.{Foo}

        test my_test() {
          trace Foo.I(42)
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownTypeConstructor { name, .. })) if name == "I" && warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn use_opaque_type_as_nested_namespace_for_constructors_fails() {
    let dependency = r#"
        pub opaque type Foo {
          I { i: Int }
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo

        test my_test() {
          trace foo.Foo.I(42)
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownTypeConstructor { name, .. })) if name == "I" && warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn use_non_imported_module_as_namespace() {
    let dependency = r#"
        pub type Foo {
          I(Int)
          B(Bool)
        }
    "#;

    let source_code = r#"
        test my_test() {
          trace foo.Foo.I(14)
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownModule { name, .. })) if name == "foo" && warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn invalid_type_field_access_chain() {
    let dependency = r#"
        pub type Foo {
          I(Int)
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo.{Foo}

        test my_test() {
          trace Foo.I.Int(42)
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::InvalidFieldAccess { .. })) if warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn invalid_type_field_access_chain_2() {
    let dependency = r#"
        pub type Foo {
          I(Int)
          B(Bool)
        }
    "#;

    let source_code = r#"
        use foo.{Foo}

        test my_test() {
          trace Foo.i(42)
          Void
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![(parse_as(dependency, "foo"))]);

    assert!(
        matches!(
            &result,
            Err((warnings, Error::UnknownTypeConstructor { .. })) if warnings.is_empty(),
        ),
        "{result:#?}"
    );
}

#[test]
fn forbid_importing_or_using_opaque_constructors() {
    let dependency = r#"
        pub opaque type Thing {
          Foo(Int)
          Bar(Int)
        }
    "#;

    let source_code = r#"
        use thing.{Thing, Foo}

        fn bar(thing: Thing) {
          expect Foo(a) = thing
          a
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))],),
        Err((_, Error::UnknownModuleField { .. })),
    ));

    let source_code = r#"
        use thing.{Thing}

        fn bar(thing: Thing) {
          expect Foo(a) = thing
          a
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![(parse_as(dependency, "thing"))],),
        Err((_, Error::UnknownTypeConstructor { .. })),
    ));
}

#[test]
fn forbid_expect_into_opaque_type_constructor_with_typecasting() {
    let source_code = r#"
        opaque type Thing {
          Foo(Int)
          Bar(Int)
        }

        fn bar(data: Data) {
          expect Foo(a): Thing = data
          a
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn allow_expect_into_nested_opaque_in_record_without_typecasting() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        type Foo { foo: Thing }

        fn bar(f: Foo) {
          expect Foo { foo: Thing { inner } } : Foo = f
          Void
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn forbid_expect_into_nested_opaque_in_record_with_typecasting() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        type Foo { foo: Thing }

        fn bar(a: Data) {
          expect Foo { foo: Thing { inner } } : Foo = a
          Void
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn forbid_expect_into_nested_opaque_in_list() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        fn bar(a: Data) {
          expect [x]: List<Thing> = [a]
          Void
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ))
}

#[test]
fn allow_expect_on_var_patterns_that_are_opaque() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        fn bar(a: Option<Thing>) {
          expect Some(thing) = a
          thing.inner
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn can_down_cast_to_data_always() {
    let source_code = r#"
        pub opaque type Foo { x: Int }
        pub fn bar(a: Foo) {
          let b: Data = a
          b
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn can_down_cast_record_containing_opaque_type_to_data() {
    let source_code = r#"
        opaque type Thing { inner: Int }
        type Bar { a0: Int, b0: Thing }

        fn bar(a: Bar) {
          let b: Data = a
          b
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn can_down_cast_to_data_on_fn_call() {
    let source_code = r#"
        pub type Foo { Foo }

        pub fn serialise(data: Data) -> ByteArray {
            ""
        }

        test foo() {
            serialise(Foo) == ""
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn can_down_cast_to_data_on_pipe() {
    let source_code = r#"
        pub type Foo { Foo }

        pub fn serialise(data: Data) -> ByteArray {
            ""
        }

        test foo() {
            (Foo |> serialise) == ""
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn correct_span_for_backpassing_args() {
    let source_code = r#"
        fn fold(list: List<a>, acc: b, f: fn(a, b) -> b) -> b {
          when list is {
            [] -> acc
            [x, ..xs] -> fold(xs, f(x, acc), f)
          }
        }

        pub fn sum(list: List<Int>) -> Int {
          let a, b <- fold(list, 0)

          a + 1
        }
    "#;

    let (warnings, _ast) = check(parse(source_code)).unwrap();

    assert!(
        matches!(&warnings[0], Warning::UnusedVariable { name, location } if name == "b" && location.start == 245 && location.end == 246)
    );
}

#[test]
fn allow_discard_for_backpassing_args() {
    let source_code = r#"
        fn fold(list: List<a>, acc: b, f: fn(a, b) -> b) -> b {
          when list is {
            [] -> acc
            [x, ..xs] -> fold(xs, f(x, acc), f)
          }
        }

        pub fn sum(list: List<Int>) -> Int {
          let a, _b <- fold(list, 0)

          a + 1
        }
    "#;

    let (warnings, _ast) = check(parse(source_code)).unwrap();

    assert_eq!(warnings.len(), 0);
}

#[test]
fn validator_private_type_leak() {
    let source_code = r#"
        type Datum {
          foo: Int,
        }

        type Redeemer {
          bar: Int,
        }

        validator bar {
          spend(datum: Option<Datum>, redeemer: Redeemer, _oref, _ctx) {
            expect Some(d) = datum

            d.foo == redeemer.bar
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::PrivateTypeLeak { .. }))
    ))
}

#[test]
fn validator_public() {
    let source_code = r#"
        pub type Datum {
          foo: Int,
        }

        pub type Redeemer {
          bar: Int,
        }

        validator bar {
          spend(datum: Option<Datum>, redeemer: Redeemer, _oref, _ctx) {
            expect Some(d) = datum

            d.foo == redeemer.bar
          }
        }
    "#;

    assert!(check_validator(parse(source_code)).is_ok())
}

#[test]
fn validator_forbid_opaque_type_in_datum_abi() {
    let source_code = r#"
        pub opaque type Datum {
          inner: Int,
        }

        validator foo {
          spend(datum: Option<Datum>, _redeemer: Data, _oref: Data, _ctx: Data) {
            True
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalOpaqueType { .. }))
    ))
}

#[test]
fn validator_forbid_opaque_type_in_redeemer_abi() {
    let source_code = r#"
        pub opaque type Redeemer {
          inner: Int,
        }

        validator foo {
          mint(redeemer: Redeemer, _policy_id: ByteArray, _ctx: Data) {
            True
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalOpaqueType { .. }))
    ))
}

#[test]
fn validator_forbid_pair_containing_opaque_type_in_redeemer_abi() {
    let source_code = r#"
        pub opaque type Thing {
          inner: Int,
        }

        validator foo {
          mint(redeemer: Pair<Thing, Int>, _policy_id: ByteArray, _ctx: Data) {
            True
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalOpaqueType { .. }))
    ))
}

#[test]
fn validator_forbid_tuple_containing_opaque_type_in_datum_abi() {
    let source_code = r#"
        pub opaque type Thing {
          inner: Int,
        }

        validator foo {
          spend(datum: Option<(Thing, Int)>, _redeemer: Data, _oref: Data, _ctx: Data) {
            True
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalOpaqueType { .. }))
    ))
}

#[test]
fn validator_forbid_opaque_type_in_data_annotation_abi() {
    let source_code = r#"
        pub opaque type Thing {
          inner: Int,
        }

        validator foo {
          spend(datum: Option<Data<Thing>>, _redeemer: Data, _oref: Data, _ctx: Data) {
            True
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalOpaqueType { .. }))
    ))
}

#[test]
fn validator_forbid_imported_record_containing_opaque_type_in_redeemer_abi() {
    let dependency = r#"
        pub opaque type Thing {
          inner: Int,
        }

        pub type Redeemer {
          pair: Pair<Thing, Int>,
        }
    "#;

    let source_code = r#"
        use thing.{Redeemer}

        validator foo {
          mint(redeemer: Redeemer, _policy_id: ByteArray, _ctx: Data) {
            True
          }
        }
    "#;

    assert!(matches!(
        check_validator_with_deps(parse(source_code), vec![parse_as(dependency, "thing")]),
        Err((_, Error::IllegalOpaqueType { .. }))
    ))
}

#[test]
fn validator_forbid_opaque_type_in_parameter_abi() {
    let source_code = r#"
        pub opaque type Config {
          threshold: Int,
        }

        validator foo(config: Config) {
          mint(_redeemer: Data, _policy_id: ByteArray, _ctx: Data) {
            config == config
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IllegalOpaqueType { position, .. })) if position == "parameter"
    ))
}

#[test]
fn validator_forbid_record_containing_opaque_type_in_parameter_abi() {
    let dependency = r#"
        pub opaque type Thing {
          inner: Int,
        }

        pub type Config {
          things: List<Thing>,
        }
    "#;

    let source_code = r#"
        use thing.{Config}

        validator foo(config: Config) {
          else(_ctx) {
            config == config
          }
        }
    "#;

    assert!(matches!(
        check_validator_with_deps(parse(source_code), vec![parse_as(dependency, "thing")]),
        Err((_, Error::IllegalOpaqueType { position, .. })) if position == "parameter"
    ))
}

#[test]
fn tuple_access_on_call() {
    let source_code = r#"
        use aiken/builtin

        pub fn list_at(xs: List<a>, index: Int) -> a {
          if index == 0 {
            builtin.head_list(xs)
          } else {
            list_at(builtin.tail_list(xs), index - 1)
          }
        }

        fn foo() {
          [list_at([(1, 2)], 0).2nd, ..[1, 2]]
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn partial_eq_call_args() {
    let source_code = r#"
        fn foo(a: Int, b: Int, c: Bool) -> Int {
            todo
        }

        fn main() -> Int {
            foo(14, 42)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IncorrectFieldsArity { .. }))
    ));
}

#[test]
fn partial_eq_callback_args() {
    let source_code = r#"
        fn foo(cb: fn(Int, Int, Bool) -> Int) -> Int {
            todo
        }

        fn main() -> Int {
            foo(fn(a, b) { a + b })
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ));
}

#[test]
fn partial_eq_callback_return() {
    let source_code = r#"
        fn foo(cb: fn(Int, Int) -> (Int, Int, Bool)) -> Int {
            todo
        }

        fn main() -> Int {
            foo(fn(a, b) { (a, b) })
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ));
}

#[test]
fn pair_access_on_call() {
    let source_code = r#"
        use aiken/builtin

        pub fn list_at(xs: List<a>, index: Int) -> a {
          if index == 0 {
            builtin.head_list(xs)
          } else {
            list_at(builtin.tail_list(xs), index - 1)
          }
        }

        fn foo() {
          [list_at([Pair(1, 2)], 0).2nd, ..[1, 2]]
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn pair_index_out_of_bound() {
    let source_code = r#"
        pub fn foo() {
            Pair(1, 2).3rd
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::PairIndexOutOfBound { .. }))
    ))
}

#[test]
fn not_indexable() {
    let source_code = r#"
        pub fn foo() {
            "foo".1st
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::NotIndexable { .. }))
    ))
}

#[test]
fn out_of_scope_access() {
    let source_code = r#"
        pub fn a(x: Int) {
          b(x)
        }

        fn b(y: Int) {
          x + y
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::UnknownVariable { .. }))
    ))
}

#[test]
fn mutually_recursive_1() {
    let source_code = r#"
        pub fn foo(x) {
            bar(x)
        }

        pub fn bar(y) {
            foo(y)
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn fn_single_variant_pattern() {
    let source_code = r#"
        pub type Foo {
            a: Int
        }

        pub fn foo(Foo { a }) {
            a + 1
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn fn_multi_variant_pattern() {
    let source_code = r#"
        type Foo {
            A { a: Int }
            B { b: Int }
        }

        pub fn foo(A { a }) {
            a + 1
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::NotExhaustivePatternMatch { .. }))
    ))
}

#[test]
fn soft_casts_from_data_to_value_is_illegal() {
    let source_code = r#"
        fn decode(data: Data) -> Bool {
          if data is value: Value {
            True
          } else {
            False
          }
        }
    "#;

    assert!(matches!(
        dbg!(check(parse(source_code))),
        Err((_, Error::IllegalSoftCast { tipo, .. })) if tipo.is_value()
    ));
}

#[test]
fn soft_casts_from_data_to_list_value_is_illegal() {
    let source_code = r#"
        fn decode(data: Data) -> Bool {
          if data is values: List<Value> {
            True
          } else {
            False
          }
        }
    "#;

    assert!(matches!(
        dbg!(check(parse(source_code))),
        Err((_, Error::IllegalSoftCast { tipo, .. })) if tipo.is_list()
    ));
}

#[test]
fn soft_casts_from_data_to_option_value_is_illegal() {
    let source_code = r#"
        fn decode(data: Data) -> Bool {
          if data is value: Option<Value> {
            True
          } else {
            False
          }
        }
    "#;

    assert!(matches!(
        dbg!(check(parse(source_code))),
        Err((_, Error::IllegalSoftCast { tipo, .. })) if tipo.is_option()
    ));
}

#[test]
fn soft_casts_from_data_to_wrapped_value_is_illegal() {
    let source_code = r#"
        type Wrapped {
          value: Value
        }

        fn decode(data: Data) -> Bool {
          if data is wrapped: Wrapped {
            True
          } else {
            False
          }
        }
    "#;

    assert!(matches!(
        dbg!(check(parse(source_code))),
        Err((_, Error::IllegalSoftCast { .. }))
    ));
}

#[test]
fn soft_casts_from_data_to_imported_generic_wrapper_is_legal() {
    let dependency = r#"
        pub type Wrapped<a> {
          Wrapped(a)
        }
    "#;

    let source_code = r#"
        use wrapped as dependency

        fn decode(data: Data) -> Bool {
          if data is wrapped: dependency.Wrapped<Int> {
            True
          } else {
            False
          }
        }
    "#;

    assert!(check_with_deps(parse(source_code), vec![parse_as(dependency, "wrapped")]).is_ok());
}

#[test]
fn soft_casts_from_data_to_imported_generic_value_wrapper_is_illegal() {
    let dependency = r#"
        pub type Wrapped<a> {
          Wrapped(a)
        }
    "#;

    let source_code = r#"
        use wrapped as dependency

        fn decode(data: Data) -> Bool {
          if data is wrapped: dependency.Wrapped<Value> {
            True
          } else {
            False
          }
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![parse_as(dependency, "wrapped")]),
        Err((_, Error::IllegalSoftCast { .. }))
    ));
}

#[test]
fn soft_cast_from_data_to_imported_opaque_value_wrapper_is_illegal() {
    let dependency = r#"
        pub opaque type Wrapped {
          Wrapped(Value)
        }
    "#;

    let source_code = r#"
        use wrapped.{Wrapped}

        fn decode(data: Data) -> Bool {
          if data is wrapped: Wrapped {
            True
          } else {
            False
          }
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![parse_as(dependency, "wrapped")]),
        Err((_, Error::ExpectOnOpaqueType { .. })),
    ));
}

#[test]
fn soft_cast_from_data_to_g1_element_is_illegal() {
    let source_code = r#"
        fn decode(data: Data) -> Bool {
          if data is point: G1Element {
            True
          } else {
            False
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalSoftCast { .. }))
    ));
}

#[test]
fn soft_cast_from_data_to_g2_element_in_a_tuple_is_illegal() {
    let source_code = r#"
        fn decode(data: Data) -> Bool {
          if data is pair: (Int, G2Element) {
            True
          } else {
            False
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalSoftCast { .. }))
    ));
}

#[test]
fn soft_cast_from_data_to_string_is_illegal() {
    let source_code = r#"
        fn decode(data: Data) -> String {
          if data is s: String {
            s
          } else {
            @"not a string"
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalSoftCast { tipo, .. })) if tipo.is_string()
    ));
}

#[test]
fn soft_cast_from_data_to_record_with_a_string_is_illegal() {
    let source_code = r#"
        type Named {
          name: String,
          age: Int,
        }

        fn decode(data: Data) -> Bool {
          if data is Named { age, .. }: Named {
            age > 0
          } else {
            False
          }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalSoftCast { .. }))
    ));
}

#[test]
fn strict_cast_from_data_to_value_is_legal() {
    let source_code = r#"
        fn decode(data: Data) -> Value {
          expect value: Value = data
          value
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn strict_cast_from_data_to_bls12_381_elements_is_legal() {
    let source_code = r#"
        type Keys {
          g1: G1Element,
          g2: List<G2Element>,
        }

        fn decode(data: Data) -> Keys {
          expect g1: G1Element = data
          expect keys: Keys = data
          Keys { ..keys, g1: g1 }
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn upcast_of_bls12_381_elements_and_value_to_data_is_legal() {
    let source_code = r#"
        fn encode(g1: G1Element, g2: G2Element, value: Value) -> List<Data> {
          let g1: Data = g1
          let g2: Data = g2
          let value: Data = value
          [g1, g2, value]
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn explicit_un_value_data_remains_legal() {
    let source_code = r#"
        use aiken/builtin

        fn decode(data: Data) -> Value {
          builtin.un_value_data(data)
        }
    "#;

    assert!(check(parse(source_code)).is_ok())
}

#[test]
fn if_soft_cast() {
    let source_code = r#"
        pub type Foo {
            a: Int
        }

        pub fn foo(foo: Data) -> Int {
          if foo is bar: Foo {
            bar.a
          } else {
            0
          }
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn if_soft_cast_sugar() {
    let source_code = r#"
        pub type Foo {
            a: Int
        }

        pub fn foo(foo: Data) -> Int {
          if foo is Foo {
            foo.a
          } else {
            0
          }
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn if_soft_cast_record() {
    let source_code = r#"
        pub type Foo {
            a: Int
        }

        pub fn foo(foo: Data) -> Int {
          if foo is Foo { a }: Foo {
            a
          } else {
            0
          }
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn if_soft_cast_no_scope_leak() {
    let source_code = r#"
        pub type Foo {
            a: Int
        }

        pub fn foo(foo: Data) -> Int {
          if foo is bar: Foo {
            bar.a
          } else {
            bar
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::UnknownVariable { name, ..  })) if name == "bar"
    ))
}

#[test]
fn if_soft_cast_no_scope_leak_2() {
    let source_code = r#"
        pub type Foo {
            a: Int
        }

        pub fn foo(foo: Data) -> Int {
          if foo is Foo { a }: Foo {
            a
          } else {
            a
          }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::UnknownVariable { name, ..  })) if name == "a"
    ))
}

#[test]
fn if_soft_cast_unused_pattern() {
    let source_code = r#"
        pub type Foo {
            a: Int
        }

        pub fn foo(foo: Data) -> Int {
          if foo is Foo { a }: Foo {
            1
          } else {
            0
          }
        }
    "#;

    let (warnings, _ast) = check(parse(source_code)).unwrap();

    assert!(matches!(
        warnings[0],
        Warning::UnusedVariable { ref name, ..  } if name == "a"
    ))
}

#[test]
fn if_soft_cast_not_data() {
    let source_code = r#"
        pub type Foo {
            Bar { a: Int }
            Buzz { b: Int }
        }

        pub fn foo(foo: Foo) -> Int {
          if foo is Bar { a }: Foo {
            a
          } else {
            0
          }
        }
    "#;

    let (warnings, _ast) = check(parse(source_code)).unwrap();

    assert!(matches!(warnings[0], Warning::UseWhenInstead { .. }))
}

#[test]
fn side_effects() {
    let source_code = r#"
        pub fn side_effects() {
            trace "Aiken, rocks!"
            Void
        }

        pub fn foo() {
            expect _ = side_effects()
            True
        }
    "#;

    let (warnings, ast) = check(parse(source_code)).unwrap();

    assert!(warnings.is_empty(), "no warnings: {warnings:#?}");

    if let Some(Definition::Fn(foo)) = ast.definitions().last() {
        if let TypedExpr::Sequence {
            ref expressions, ..
        } = foo.body
        {
            matches!(
                expressions[..],
                [
                    TypedExpr::Assignment {
                        pattern: Pattern::Discard { .. },
                        ..
                    },
                    TypedExpr::Var { .. },
                ],
            );
        } else {
            unreachable!();
        }
    } else {
        unreachable!();
    }
}

#[test]
fn pattern_bytearray() {
    let source_code = r#"
        pub fn main(foo: ByteArray) {
            when foo is {
                #[1, 2, 3] -> True
                #"00ff" -> True
                "Aiken, rocks!" -> True
                _ -> False
            }
        }
    "#;

    let result = check(parse(source_code));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert!(warnings.is_empty(), "no warnings: {warnings:#?}");
}

#[test]
fn pattern_bytearray_not_unify_clause_list() {
    let source_code = r#"
        pub fn main(foo: ByteArray) {
            when foo is {
                [1, 2, 3] -> True
                _ -> False
            }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn pattern_bytearray_not_unify_clause_int() {
    let source_code = r#"
        pub fn main(foo: ByteArray) {
            when foo is {
                42 -> True
                _ -> False
            }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn pattern_bytearray_not_unify_subject() {
    let source_code = r#"
        pub fn main(foo: String) {
            when foo is {
                "42" -> True
                _ -> False
            }
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn recover_no_assignment_sequence() {
    let source_code = r#"
        pub fn main() {
            let result = 42
            expect result + 1 == 43
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn recover_no_assignment_fn_body() {
    let source_code = r#"
        pub fn is_bool(foo: Data) -> Void {
            expect _: Bool = foo
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn recover_no_assignment_when_clause() {
    let source_code = r#"
        pub fn main(foo) {
            when foo is {
                [] -> Void
                [x, ..] -> expect _: Int = x
            }
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn recover_no_assignment_fn_if_then_else() {
    let source_code = r#"
        pub fn foo(weird_maths) -> Void {
            if weird_maths {
                expect 1 == 2
            } else {
                expect 1 + 1 == 2
            }
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn test_return_explicit_void() {
    let source_code = r#"
        test foo() {
            Void
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn test_return_implicit_void() {
    let source_code = r#"
        test foo() {
            let data: Data = 42
            expect _: Int = data
        }
    "#;

    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn test_return_illegal() {
    let source_code = r#"
        test foo() {
            42
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalTestType { .. }))
    ))
}

#[test]
fn validator_by_name() {
    let source_code = r#"
        validator foo {
            mint(_redeemer: Data, policy_id: ByteArray, _self: Data) {
                policy_id == "foo"
            }
        }

        test test_1() {
            foo.mint(Void, "foo", Void)
        }
    "#;

    assert!(check_validator(parse(source_code)).is_ok())
}

#[test]
fn validator_by_name_with_params() {
    let source_code = r#"
        validator foo(_thing: Data) {
            mint(_redeemer: Data, policy_id: ByteArray, _self: Data) {
                policy_id == "foo"
            }
        }

        test test_1() {
            foo.mint(Void, Void, "foo", Void)
        }
    "#;

    assert!(check_validator(parse(source_code)).is_ok())
}

#[test]
fn validator_by_name_unknown_handler() {
    let source_code = r#"
        validator foo {
            mint(_redeemer: Data, policy_id: ByteArray, _self: Data) {
                policy_id == "foo"
            }
        }

        test foo() {
            foo.bar(Void, "foo", Void)
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::UnknownValidatorHandler { .. }))
    ))
}

#[test]
fn validator_by_name_module_duplicate() {
    let source_code = r#"
        use aiken/builtin

        validator builtin {
            mint(_redeemer: Data, _policy_id: ByteArray, _self: Data) {
                True
            }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::DuplicateName { .. }))
    ))
}

#[test]
fn validator_by_name_validator_duplicate_1() {
    let source_code = r#"
        validator foo {
            mint(_redeemer: Data, _policy_id: ByteArray, _self: Data) {
                True
            }
        }

        validator foo {
            mint(_redeemer: Data, _policy_id: ByteArray, _self: Data) {
                True
            }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::DuplicateName { .. }))
    ))
}

#[test]
fn validator_by_name_validator_duplicate_2() {
    let source_code = r#"
        validator foo {
            mint(_redeemer: Data, _policy_id: ByteArray, _self: Data) {
                True
            }

            mint(_redeemer: Data, _policy_id: ByteArray, _self: Data) {
                True
            }
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::DuplicateName { .. }))
    ))
}

#[test]
fn exhaustive_handlers() {
    let source_code = r#"
            validator foo {
              mint(_redeemer, _policy_id, _self) {
                True
              }

              spend(_datum, _redeemer, _policy_id, _self) {
                True
              }

              withdraw(_redeemer, _account, _self) {
                True
              }

              publish(_redeemer, _certificate, _self) {
                True
              }

              vote(_redeemer, _voter, _self) {
                True
              }

              propose(_redeemer, _proposal, _self) {
                True
              }
            }
        "#;

    assert!(check_validator(parse(source_code)).is_ok())
}

#[test]
fn extraneous_fallback_on_exhaustive_handlers() {
    let source_code = r#"
            validator foo {
              mint(_redeemer, _policy_id, _self) {
                True
              }

              spend(_datum, _redeemer, _policy_id, _self) {
                True
              }

              withdraw(_redeemer, _account, _self) {
                True
              }

              publish(_redeemer, _certificate, _self) {
                True
              }

              vote(_redeemer, _voter, _self) {
                True
              }

              propose(_redeemer, _proposal, _self) {
                True
              }

              else (_) -> Bool {
                fail
              }
            }
        "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::UnexpectedValidatorFallback { .. }))
    ))
}

#[test]
fn constant_usage() {
    let source_code = r#"
        pub const some_bool_constant: Bool = True

        const some_int_constant: Int = 42

        const some_string_constant: String = @"Aiken"

        test foo() {
          some_int_constant == 42
        }
    "#;

    let result = check(parse(source_code));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert!(matches!(
        &warnings[..],
        [Warning::UnusedPrivateModuleConstant {
            name,
            ..
        }] if name == "some_string_constant"
    ));
}

#[test]
fn wrong_arity_on_known_builtin() {
    let source_code = r#"
        const foo: Option<Int> = Some()
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::IncorrectFunctionCallArity { .. }))
    ))
}

#[test]
fn softcasting_unused_let_binding() {
    let source_code = r#"
        pub fn is_int(data: Data) -> Bool {
          if data is Int {
            True
          } else {
            False
          }
        }
    "#;

    let result = dbg!(check(parse(source_code)));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert!(warnings.is_empty(), "should not contain any warnings");
}

#[test]
fn dangling_let_in_block() {
    let source_code = r#"
        fn for_each(xs: List<Int>, with: fn(Int) -> a) -> a {
            todo
        }

        test foo() {
            {
                let _ <- for_each([1, 2, 3])
            }
        }
    "#;

    let result = check_validator(parse(source_code));

    assert!(
        matches!(result, Err((_, Error::LastExpressionIsAssignment { .. }))),
        "{result:?}"
    )
}

#[test]
fn default_trace_return() {
    let source_code = r#"
        fn debug() {
          trace @"patate": Void
        }

        test foo() {
            debug()
            True
        }
    "#;

    assert!(matches!(check_validator(parse(source_code)), Ok(..)))
}

#[test]
fn dangling_trace_let_standalone() {
    let source_code = r#"
        test foo() {
          trace @"foo"
          let True = True
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::LastExpressionIsAssignment { .. }))
    ))
}

#[test]
fn dangling_trace_let_in_sequence() {
    let source_code = r#"
        test foo() {
          let predicate = True
          trace @"foo"
          let result = predicate
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::LastExpressionIsAssignment { .. }))
    ))
}

#[test]
fn dangling_trace_let_in_trace() {
    let source_code = r#"
        test foo() {
          trace @"foo"
          trace @"bar"
          let result = True
        }
    "#;

    assert!(matches!(
        check_validator(parse(source_code)),
        Err((_, Error::LastExpressionIsAssignment { .. }))
    ))
}

#[test]
fn destructuring_validator_params_tuple() {
    let source_code = r#"
        validator foo((x, y): (Int, Int)) {
            mint(_redeemer, _policy_id, _self) {
              x + y > 42
            }

            else(_) {
              fail
            }
        }
    "#;

    let result = check_validator(parse(source_code));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert!(
        matches!(&warnings[..], &[]),
        "should be empty: {warnings:#?}"
    );
}

#[test]
fn destructuring_validator_params_record() {
    let source_code = r#"
        pub type Foo {
            Foo(Int, Int)
        }

        validator foo(Foo(x, y): Foo) {
            mint(_redeemer, _policy_id, _self) {
              x + y > 42
            }

            else(_) {
              fail
            }
        }
    "#;

    let result = check_validator(parse(source_code));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert!(
        matches!(&warnings[..], &[]),
        "should be empty: {warnings:#?}"
    );
}

#[test]
fn constant_generic_lambda() {
    let source_code = r#"const foo: fn(a) -> List<a> = fn(x: a) { [x] }"#;
    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::GenericLeftAtBoundary { .. }))
    ))
}

#[test]
fn constant_generic_mismatch() {
    let source_code = r#"const foo: List<a> = [42]"#;
    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn constant_generic_inferred_1() {
    let source_code = r#"const foo = [42]"#;
    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn constant_generic_inferred_2() {
    let source_code = r#"
        const foo = fn(x) { [x] }(42)

        test my_test() {
            foo == [42]
        }
    "#;
    assert!(check(parse(source_code)).is_ok());
}

#[test]
fn constant_generic_inferred_3() {
    let source_code = r#"const foo: List<a> = fn(x) { [x] }(42)"#;
    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn constant_generic_empty() {
    let source_code = r#"const foo: List<a> = []"#;
    assert!(check_validator(parse(source_code)).is_ok());
}

#[test]
fn unused_record_fields_1() {
    let source_code = r#"
        pub type Foo {
          Foo { field0: Bool, field1: Bool }
          Bar
        }

        test confusing() {
          let foo = Foo(True, True)
          when foo is {
            Foo(field1, field0) -> field0 || field1
            _ -> False
          }
        }
    "#;

    let result = check_validator(parse(source_code));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert_eq!(
        warnings[0],
        Warning::UnusedRecordFields {
            location: Span::create(193, 19),
            suggestion: UntypedPattern::Constructor {
                is_record: true,
                location: Span::create(193, 19),
                name: "Foo".to_string(),
                arguments: vec![
                    CallArg {
                        label: Some("field0".to_string()),
                        location: Span::create(197, 6),
                        value: Pattern::Var {
                            location: Span::create(197, 6),
                            name: "field1".to_string()
                        }
                    },
                    CallArg {
                        label: Some("field1".to_string()),
                        location: Span::create(205, 6),
                        value: Pattern::Var {
                            location: Span::create(205, 6),
                            name: "field0".to_string()
                        }
                    }
                ],
                module: None,
                constructor: (),
                spread_location: None,
                tipo: ()
            }
        }
    );
}

#[test]
fn unused_record_fields_2() {
    let source_code = r#"
        pub type Foo {
          Foo { field0: Bool, field1: Bool }
          Bar
        }

        test confusing() {
          let foo = Foo(True, True)
          when foo is {
            Foo(field0, field1) -> field0 || field1
            _ -> False
          }
        }
    "#;

    let result = check_validator(parse(source_code));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert_eq!(
        warnings[0],
        Warning::UnusedRecordFields {
            location: Span::create(193, 19),
            suggestion: UntypedPattern::Constructor {
                is_record: true,
                location: Span::create(193, 19),
                name: "Foo".to_string(),
                arguments: vec![
                    CallArg {
                        label: Some("field0".to_string()),
                        location: Span::create(197, 6),
                        value: Pattern::Var {
                            location: Span::create(197, 6),
                            name: "field0".to_string()
                        }
                    },
                    CallArg {
                        label: Some("field1".to_string()),
                        location: Span::create(205, 6),
                        value: Pattern::Var {
                            location: Span::create(205, 6),
                            name: "field1".to_string()
                        }
                    }
                ],
                module: None,
                constructor: (),
                spread_location: None,
                tipo: ()
            }
        }
    );
}

#[test]
fn unused_record_fields_3() {
    let source_code = r#"
        pub type Foo {
          Foo { field0: Bool, field1: Bool }
          Bar
        }

        test confusing() {
          let foo = Foo(True, True)
          when foo is {
            Foo(a, _) -> a
            _ -> False
          }
        }
    "#;

    let result = check_validator(parse(source_code));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert_eq!(
        warnings[0],
        Warning::UnusedRecordFields {
            location: Span::create(193, 9),
            suggestion: UntypedPattern::Constructor {
                is_record: true,
                location: Span::create(193, 9),
                name: "Foo".to_string(),
                arguments: vec![CallArg {
                    label: Some("field0".to_string()),
                    location: Span::create(197, 6),
                    value: Pattern::Var {
                        location: Span::create(197, 1),
                        name: "a".to_string()
                    }
                },],
                module: None,
                constructor: (),
                spread_location: Some(Span::create(199, 2)),
                tipo: ()
            }
        }
    );
}

#[test]
fn unused_record_fields_4() {
    let source_code = r#"
        pub type Foo {
          Foo { field0: Bool, field1: Bool }
          Bar
        }

        test confusing() {
          let foo = Foo(True, True)
          when foo is {
            Foo(a, ..) -> a
            _ -> False
          }
        }
    "#;

    let result = check_validator(parse(source_code));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert_eq!(
        warnings[0],
        Warning::UnusedRecordFields {
            location: Span::create(193, 10),
            suggestion: UntypedPattern::Constructor {
                is_record: true,
                location: Span::create(193, 10),
                name: "Foo".to_string(),
                arguments: vec![CallArg {
                    label: Some("field0".to_string()),
                    location: Span::create(197, 6),
                    value: Pattern::Var {
                        location: Span::create(197, 1),
                        name: "a".to_string()
                    }
                },],
                module: None,
                constructor: (),
                spread_location: Some(Span::create(200, 2)),
                tipo: ()
            }
        }
    );
}

#[test]
fn unused_record_fields_5() {
    let source_code = r#"
        pub type Foo {
          b_is_before_a: Bool,
          a_is_after_b: Bool,
        }

        test confusing() {
          let Foo(a, b) = Foo(True, True)
          a || b
        }
    "#;

    let result = check_validator(parse(source_code));
    assert!(result.is_ok());

    let (warnings, _) = result.unwrap();
    assert_eq!(
        warnings[0],
        Warning::UnusedRecordFields {
            location: Span::create(137, 9),
            suggestion: UntypedPattern::Constructor {
                is_record: true,
                location: Span::create(137, 9),
                name: "Foo".to_string(),
                arguments: vec![
                    CallArg {
                        label: Some("b_is_before_a".to_string()),
                        location: Span::create(141, 13),
                        value: Pattern::Var {
                            location: Span::create(141, 1),
                            name: "a".to_string()
                        }
                    },
                    CallArg {
                        label: Some("a_is_after_b".to_string()),
                        location: Span::create(144, 12),
                        value: Pattern::Var {
                            location: Span::create(144, 1),
                            name: "b".to_string()
                        }
                    }
                ],
                module: None,
                constructor: (),
                spread_location: None,
                tipo: ()
            }
        }
    );
}

#[test]
fn as_data() {
    let source_code = r#"
        type Foo {
            foo: Int
        }

        type Bar {
            bar: Int
        }

        test foo() {
            [as_data(Foo(14)), as_data(Bar(42))] != []
        }
    "#;

    assert!(matches!(check_validator(parse(source_code)), Ok(..)))
}

#[test]
fn unused_constructors() {
    let source_code = r#"
        type Foo {
          A
          B
        }

        type Bar {
          C
          D
        }

        pub fn to_string() -> String {
          if C == C {
            @"C"
          } else {
            @""
          }
        }
    "#;

    let (warnings, _) = check_validator(parse(source_code)).unwrap();

    assert!(
        matches!(
            warnings.as_slice(),
            [
                Warning::UnusedType { name: unused_type, .. },
                Warning::UnusedConstructor { name: unused_constr, .. }
            ]
            if unused_type == "Foo" && unused_constr == "D",
        ),
        "{warnings:#?}"
    );
}

#[test]
fn decorator_validation_encoding_on_enum() {
    let source_code = r#"
        @list
        pub type Redeemer {
          Buy
          Cancel
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::DecoratorValidation { .. }))
    ))
}

#[test]
fn decorator_validation_encoding_on_enum_member() {
    let source_code = r#"
        pub type Redeemer {
          @list
          Buy
          Cancel
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::DecoratorValidation { .. }))
    ))
}

#[test]
fn decorator_validation_tag_on_enum() {
    let source_code = r#"
        @tag(12)
        pub type Redeemer {
          Buy
          Cancel
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::DecoratorValidation { .. }))
    ))
}

#[test]
fn decorator_validation_conflict() {
    let source_code = r#"
        @list
        @tag(100)
        pub type Datum {
          thing: Int,
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ConflictingDecorators { .. }))
    ))
}

#[test]
fn decorator_validation_legit_record_tag() {
    let source_code = r#"
        @tag(100)
        pub type Datum {
          thing: Int,
        }
    "#;

    assert!(dbg!(check(parse(source_code))).is_ok())
}

#[test]
fn decorator_validation_legit_record_list() {
    let source_code = r#"
        @list
        pub type Datum {
          thing: Int,
        }
    "#;

    assert!(dbg!(check(parse(source_code))).is_ok())
}

#[test]
fn decorator_validation_legit_enum() {
    let source_code = r#"
        pub type Redeemer {
          @tag(4)
          Buy
          @tag(10)
          Cancel
        }
    "#;

    assert!(dbg!(check(parse(source_code))).is_ok())
}

#[test]
fn decorator_validation_overlaping_tags_explicit() {
    let source_code = r#"
        pub type Redeemer {
          @tag(4)
          Buy
          @tag(4)
          Cancel
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::DecoratorTagOverlap { .. }))
    ))
}

#[test]
fn decorator_validation_overlaping_tags() {
    let source_code = r#"
        pub type Redeemer {
          @tag(1)
          Buy
          Cancel
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::DecoratorTagOverlap { .. }))
    ))
}

#[test]
fn validator_parameter_scope_escape() {
    let source_code = r#"
        validator placeholder(foo: Option<Int>) {
          withdraw(_redeemer: Data, _credential: Data, self: Data) {
            bar(self)
          }
        }

        fn bar(_self: Data) {
          // foo shouldn't be in scope here!
          when foo is {
            None -> True
            Some(_) -> False
          }
        }
    "#;

    assert!(matches!(
        dbg!(check_validator(parse(source_code))),
        Err((_, Error::UnknownVariable { name, .. })) if name == "foo"
    ))
}

#[test]
fn backpassing_with_labels() {
    let source_code = r#"
        fn foo(n: Int, return: fn(Int) -> a) -> a {
          return(n)
        }

        test using_foo() {
          let n <- foo(n: 42)
          n == 42
        }
    "#;

    assert!(dbg!(check_validator(parse(source_code))).is_ok())
}

#[test]
fn dangling_assignment_in_trace_arg() {
    let source_code = r#"
        test foo() {
          trace @"what's going on?": expect 1 + 1 == 2
        }
    "#;

    assert!(dbg!(check(parse(source_code))).is_ok())
}

#[test]
fn illegal_tracing_argument() {
    let source_code = r#"
        use aiken/builtin

        const q: G1Element =
          #<Bls12_381, G1>"97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb"

        const p: G2Element =
          #<Bls12_381, G2>"93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8"

        test foo() {
          let some_miller_loop_result = builtin.bls12_381_miller_loop(q, p)
          trace @"illegal trace": some_miller_loop_result
          Void
        }
    "#;

    assert!(matches!(
        dbg!(check_validator(parse(source_code))),
        Err((_, Error::IllegalTraceArgument { .. }))
    ))
}

#[test]
fn pointfree_pair() {
    let source_code = r#"
        fn mk_pair(constructor: fn(a, b) -> Pair<a, b>, a: a, b: b) -> Pair<a, b> {
          constructor(a, b)
        }

        test foo() {
          mk_pair(Pair, 14, "42") == Pair(14, "42")
        }
    "#;

    assert!(dbg!(check_validator(parse(source_code))).is_ok())
}

#[test]
fn incomplete_pair() {
    let source_code = r#"
        test foo() {
          Pair(14, "42") == Pair(14)
        }
    "#;

    assert!(matches!(
        dbg!(check_validator(parse(source_code))),
        Err((_, Error::IncorrectFunctionCallArity { expected, .. })) if expected == 2
    ))
}

#[test]
fn coerce_data_into_record() {
    let source_code = r#"
        type Datum { owner: ByteArray, amount: Int }

        fn amount(data: Data) -> Int {
          coerce datum: Datum = data
          datum.amount
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn coerce_data_into_opaque_type() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        fn bar(n: Data) -> Thing {
          coerce a: Thing = n
          a
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn coerce_into_imported_opaque_type() {
    let dependency = r#"
        pub opaque type Dict<key, value> { inner: Pairs<ByteArray, value> }

        pub opaque type Assets { inner: Dict<ByteArray, Dict<ByteArray, Int>> }
    "#;

    let source_code = r#"
        use cardano/assets.{Assets}

        fn from_pairs(xs: Pairs<ByteArray, Pairs<ByteArray, Int>>) -> Assets {
          coerce assets: Assets = xs
          assets
        }

        fn to_pairs(assets: Assets) -> Pairs<ByteArray, Pairs<ByteArray, Int>> {
          coerce xs: Pairs<ByteArray, Pairs<ByteArray, Int>> = assets
          xs
        }
    "#;

    let result = check_with_deps(
        parse(source_code),
        vec![parse_as(dependency, "cardano/assets")],
    );

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn coerce_from_function() {
    let source_code = r#"
        fn bar() -> Int {
          coerce n: Int = fn(x: Int) { x + 1 }
          n
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalCoerce { .. }))
    ))
}

#[test]
fn coerce_into_function() {
    let source_code = r#"
        fn bar(data: Data) -> Int {
          coerce f: fn(Int) -> Int = data
          f(1)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalCoerce { .. }))
    ))
}

#[test]
fn coerce_into_value() {
    let source_code = r#"
        fn bar(data: Data) -> Value {
          coerce value: Value = data
          value
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn coerce_empty_list() {
    let source_code = r#"
        fn bar() -> Int {
          coerce n: Int = []
          n
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::AmbiguousEmptyList { .. }))
    ))
}

#[test]
fn coerce_binding_has_the_annotated_type() {
    let source_code = r#"
        type Datum { owner: ByteArray, amount: Int }

        fn bar(data: Data) -> ByteArray {
          coerce datum: Datum = data
          datum
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn function_argument_types_match_exactly_when_passed() {
    let source_code = r#"
        fn apply(f: fn(Data) -> Bool, data: Data) -> Bool {
          f(data)
        }

        fn is_positive(n: Int) -> Bool {
          n > 0
        }

        fn main(data: Data) -> Bool {
          apply(is_positive, data)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn function_argument_types_match_exactly_in_expect() {
    let source_code = r#"
        fn is_unit(data: Data) -> Bool {
          data == Void
        }

        fn main() -> Bool {
          expect f: fn(Int) -> Bool = is_unit
          f(42)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn function_argument_types_match_exactly_in_a_generic_list() {
    let source_code = r#"
        fn any(xs: List<a>, predicate: fn(a) -> Bool) -> Bool {
          when xs is {
            [] -> False
            [x, ..rest] -> predicate(x) || any(rest, predicate)
          }
        }

        fn is_positive(n: Int) -> Bool {
          n > 0
        }

        fn main(xs: List<Data>) -> Bool {
          any(xs, is_positive)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn pipe_does_not_cast_function_arguments() {
    let source_code = r#"
        fn apply(f: fn(Data) -> Bool) -> Bool {
          f(Void)
        }

        fn is_positive(n: Int) -> Bool {
          n > 0
        }

        fn main() -> Bool {
          is_positive |> apply
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((
            _,
            Error::CouldNotUnify {
                situation: Some(UnifyErrorSituation::PipeTypeMismatch),
                ..
            }
        ))
    ))
}

/// A cast that meets a type whose type isn't known yet allows it, as long as it turns out to be a
/// type the cast can convert once its definition is inferred. What's still unknown then is linked
/// to Data.
#[test]
fn upcast_refuses_an_unknown_type_that_becomes_a_function() {
    let source_code = r#"
        fn main() -> Data {
          let to_data = fn(x) {
            let data: Data = x
            data
          }

          to_data(fn(n: Int) { n + 1 })
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn expect_refuses_an_unknown_type_that_becomes_a_function() {
    let source_code = r#"
        fn main() -> Data {
          let to_data = fn(x) {
            expect data: Data = x
            data
          }

          to_data(fn(n: Int) { n + 1 })
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

// The refusal is reported by the coerce, not as a type mismatch: the cast is checked once the
// type is known, and a coerce never unifies its value with anything.
#[test]
fn coerce_refuses_an_unknown_type_that_becomes_a_function() {
    let source_code = r#"
        fn main() -> Int {
          let to_int = fn(x) {
            coerce n: Int = x
            n
          }

          to_int(fn(n: Int) { n + 1 })
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalCoerce { .. }))
    ))
}

#[test]
fn upcast_refuses_an_unknown_type_that_becomes_a_miller_loop_result() {
    let source_code = r#"
        use aiken/builtin

        fn main(g1: G1Element, g2: G2Element) -> Data {
          let to_data = fn(x) {
            let data: Data = x
            data
          }

          to_data(builtin.bls12_381_miller_loop(g1, g2))
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn coerce_refuses_an_unknown_type_that_becomes_a_miller_loop_result() {
    let source_code = r#"
        use aiken/builtin

        fn main(g1: G1Element, g2: G2Element) -> Int {
          let to_int = fn(x) {
            coerce n: Int = x
            n
          }

          to_int(builtin.bls12_381_miller_loop(g1, g2))
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalCoerce { .. }))
    ))
}

/// `expect xs: List<_>` doesn't know its element type yet, so it can't tell the cast is opaque.
/// The element type becomes `Secret` after, which the cast is refused for.
#[test]
fn expect_refuses_an_unknown_type_argument_that_becomes_opaque() {
    let local = r#"
        opaque type Secret { inner: Int }

        fn main(data: Data) -> List<Secret> {
          expect xs: List<_> = data
          let hidden: List<Secret> = xs
          hidden
        }
    "#;

    let dependency = r#"
        pub opaque type Secret { inner: Int }
    "#;

    let imported = r#"
        use secret.{Secret}

        fn main(data: Data) -> List<Secret> {
          expect xs: List<_> = data
          let hidden: List<Secret> = xs
          hidden
        }
    "#;

    assert!(matches!(
        check(parse(local)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ));

    assert!(matches!(
        check_with_deps(parse(imported), vec![parse_as(dependency, "secret")]),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ));
}

#[test]
fn soft_cast_refuses_an_unknown_type_argument_that_becomes_a_string() {
    let source_code = r#"
        fn main(data: Data) -> Int {
          if data is xs: List<_> {
            let strings: List<String> = xs
            list_length(strings)
          } else {
            0
          }
        }

        fn list_length(xs: List<a>) -> Int {
          todo
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalSoftCast { .. }))
    ))
}

/// Once the element type is known, the cast is the one a `List<Int>` annotation would make.
#[test]
fn expect_accepts_an_unknown_type_argument_that_becomes_known() {
    let source_code = r#"
        fn main(data: Data) -> List<Int> {
          expect xs: List<_> = data
          let ys: List<Int> = xs
          ys
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn a_type_still_unknown_at_the_end_of_its_definition_is_linked_to_data() {
    let source_code = r#"
        pub fn main(data: Data) {
          expect xs: List<_> = data
          xs
        }
    "#;

    let (_, module) = check(parse(source_code)).expect("should type-check");

    let tipo = &module.type_info.values["main"].tipo;

    assert_eq!(tipo.to_pretty(0), "fn(Data) -> List<Data>");
}

#[test]
fn captures_and_lambdas_that_upcast_their_argument_can_be_passed_to_higher_order_functions() {
    let prelude = r#"
        fn eq_data(d: Data, n: Int) -> Bool {
          todo
        }

        fn serialise(d: Data) -> ByteArray {
          todo
        }

        fn all(xs: List<a>, f: fn(a) -> Bool) -> Bool {
          todo
        }

        fn map(xs: List<a>, f: fn(a) -> b) -> List<b> {
          todo
        }

        fn apply(f: fn(Int) -> Bool) -> Bool {
          f(1)
        }
    "#;

    let cases = [
        // A capture.
        "all([1, 1], eq_data(_, 1))",
        // A capture passed to a function of the module.
        "apply(eq_data(_, 1))",
        // A capture of a function that takes Data.
        "map([1, 2], serialise(_)) == [#\"01\", #\"02\"]",
        // A lambda bound by `let`, which is only used afterwards.
        "{ let is_one = fn(x) { eq_data(x, 1) }\n all([1, 1], is_one) }",
        // The same, inline.
        "all([1, 1], fn(x) { eq_data(x, 1) })",
        // The same, piped.
        "1 |> eq_data(_, 1)",
    ];

    for case in cases {
        let source_code = format!("{prelude}\nfn main() -> Bool {{\n{case}\n}}\n");

        let result = check(parse(&source_code));

        assert!(result.is_ok(), "{case}: {result:#?}");
    }
}

/// `==` compares through Data, and functions and `MillerLoopResult` have no Data form. A type
/// variable that meets `==` can't become one later, whether it's known by the end of the
/// definition (a lambda's argument) or a generic of a function that compares it, directly or
/// through another function, and is instantiated with one.
#[test]
fn equality_refuses_types_without_a_data_form_through_type_variables() {
    for source_code in [
        r#"
        fn same(x: a, y: a) -> Bool {
          x == y
        }

        fn main() -> Bool {
          same(fn(n: Int) { n }, fn(n: Int) { n })
        }
        "#,
        r#"
        fn main() -> Bool {
          let same = fn(x, y) { x == y }
          same(fn(n: Int) { n }, fn(n: Int) { n })
        }
        "#,
        r#"
        use aiken/builtin

        fn same(x: a, y: a) -> Bool {
          x == y
        }

        fn main(p: G1Element, q: G2Element) -> Bool {
          let ml = builtin.bls12_381_miller_loop(p, q)
          same(ml, ml)
        }
        "#,
        r#"
        fn same(x: a, y: a) -> Bool {
          x != y
        }

        fn same_again(x: b, y: b) -> Bool {
          same(x, y)
        }

        fn main() -> Bool {
          same_again(fn(n: Int) { n }, fn(n: Int) { n })
        }
        "#,
        r#"
        fn comparer() -> fn(a, a) -> Bool {
          fn(x, y) { x == y }
        }

        fn main() -> Bool {
          comparer()(fn(n: Int) { n }, fn(n: Int) { n })
        }
        "#,
        r#"
        fn same(x: a, y: a) -> Bool {
          x == y
        }

        fn apply(f: fn(b, b) -> Bool, x: b) -> Bool {
          f(x, x)
        }

        fn main() -> Bool {
          apply(same, fn(n: Int) { n })
        }
        "#,
        r#"
        fn other(x: a, y: a) -> Bool {
          same(x, y, False)
        }

        fn same(x: a, y: a, b: Bool) -> Bool {
          if b { other(x, y) } else { x == y }
        }

        fn main() -> Bool {
          other(fn(n: Int) { n }, fn(n: Int) { n })
        }
        "#,
        r#"
        fn main() -> Bool {
          same(fn(n: Int) { n }, fn(n: Int) { n }, True)
        }

        fn same(x: a, y: a, b: Bool) -> Bool {
          if b { other(x, y) } else { x == y }
        }

        fn other(x: a, y: a) -> Bool {
          same(x, y, False) && main()
        }
        "#,
        r#"
        fn same(x: a, y: a) -> Bool {
          x == y
        }

        fn id(n: Int) -> Int {
          n
        }

        const compared: Bool = same(id, id)
        "#,
        r#"
        fn same(x: a, y: a) -> Bool {
          x == y
        }

        fn id(n: Int) -> Int {
          n
        }

        fn pick(b: Bool) -> Fuzzer<Int> {
          fn(prng) { if b { Some((prng, 1)) } else { None } }
        }

        test prop(n via pick(same(id, id))) {
          n == 1
        }
        "#,
    ] {
        let result = check(parse(source_code));

        assert!(
            matches!(result, Err((_, Error::IllegalComparison { .. }))),
            "{source_code}\n{result:#?}"
        );
    }

    let dependency = r#"
        pub fn same(x: a, y: a) -> Bool {
          x == y
        }
    "#;

    let source_code = r#"
        use dep

        fn main() -> Bool {
          dep.same(fn(n: Int) { n }, fn(n: Int) { n })
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![parse_as(dependency, "dep")]);

    assert!(
        matches!(result, Err((_, Error::IllegalComparison { .. }))),
        "{result:#?}"
    );
}

/// The check above only refuses types without a Data form: comparing through type variables
/// that end up with one still type-checks.
#[test]
fn equality_through_type_variables_accepts_types_with_a_data_form() {
    let dependency = r#"
        pub fn same(x: a, y: a) -> Bool {
          x == y
        }
    "#;

    let source_code = r#"
        use dep

        fn same_again(x: b, y: b) -> Bool {
          dep.same(x, y)
        }

        fn has(xs: List<a>, x: a) -> Bool {
          when xs is {
            [] -> False
            [head, ..tail] -> head == x || has(tail, x)
          }
        }

        fn main(g: G1Element) -> Bool {
          let same = fn(x, y) { x == y }
          and {
            same_again(1, 1),
            dep.same([#"00"], []),
            has([Some(g)], None),
            same(as_data(1), as_data(1)),
          }
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![parse_as(dependency, "dep")]);

    assert!(result.is_ok(), "{result:#?}");
}

/// casts.mdx: "An empty list `[]` whose element type is unknown is ambiguous and refused" (its Data
/// form is `List []` as a `List<a>` and `Map []` as `Pairs<k, v>`). That holds for a list nested
/// in a type argument, a tuple or a pair too, in an upcast and in a `coerce`.
#[test]
fn upcast_refuses_a_nested_empty_list_of_unknown_type() {
    for value in ["Some([])", "([], 1)", "Pair(1, [])", "[[]]"] {
        for source_code in [
            format!("fn main() -> Data {{\n  let d: Data = {value}\n  d\n}}"),
            format!("fn main() -> Int {{\n  coerce n: Int = {value}\n  n\n}}"),
        ] {
            let result = check(parse(&source_code));

            assert!(
                matches!(result, Err((_, Error::AmbiguousEmptyList { .. }))),
                "{source_code}\n{result:#?}"
            );
        }
    }
}

#[test]
fn local_and_imported_opaque_wrappers_cast_the_same_way() {
    let dependency = r#"
        pub opaque type Wrapped {
          Wrapped(Value)
        }

        pub opaque type Point {
          Point(G1Element)
        }
    "#;

    let casts = [
        "fn cast(data: Data) -> Wrapped { coerce w: Wrapped = data\n w }",
        "fn cast(w: Wrapped) -> Data { coerce d: Data = w\n d }",
        "fn cast(w: Wrapped) -> Data { let d: Data = w\n d }",
        "fn cast(data: Data) -> Wrapped { expect w: Wrapped = data\n w }",
        "fn cast(data: Data) -> Point { coerce p: Point = data\n p }",
        "fn cast(p: Point) -> Data { let d: Data = p\n d }",
    ];

    for cast in casts {
        let local = format!(
            "opaque type Wrapped {{ Wrapped(Value) }}\nopaque type Point {{ Point(G1Element) }}\n{cast}"
        );
        let imported = format!("use wrapped.{{Wrapped, Point}}\n{cast}");

        let local = check(parse(&local)).map(|_| ()).map_err(|(_, e)| e);
        let imported = check_with_deps(parse(&imported), vec![parse_as(dependency, "wrapped")])
            .map(|_| ())
            .map_err(|(_, e)| e);

        match (&local, &imported) {
            (Ok(()), Ok(())) => assert!(!cast.contains("expect"), "{cast}"),
            (Err(Error::ExpectOnOpaqueType { .. }), Err(Error::ExpectOnOpaqueType { .. })) => {
                assert!(cast.contains("expect"), "{cast}")
            }
            _ => panic!("{cast}\nlocal: {local:#?}\nimported: {imported:#?}"),
        }
    }
}

#[test]
fn expect_into_opaque_type_through_a_type_link() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        fn id(x: a) -> a {
          x
        }

        fn bar(t: Thing) -> Thing {
          expect x: Thing = id(t)
          x
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn expect_into_opaque_type_from_a_non_data_type_is_a_type_mismatch() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        fn bar() -> Thing {
          expect x: Thing = 1
          x
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn expect_into_opaque_type_from_a_different_container_is_a_type_mismatch() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        fn bar(data: Data) -> Option<Thing> {
          expect x: Option<Thing> = [data]
          x
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::CouldNotUnify { .. }))
    ))
}

#[test]
fn expect_into_opaque_type_in_a_tuple_is_an_opaque_cast_only_from_data() {
    let from_data = r#"
        opaque type Thing { inner: Int }

        fn bar(data: Data) -> (Thing, Int) {
          expect x: (Thing, Int) = (data, 1)
          x
        }
    "#;

    let from_a_longer_tuple = r#"
        opaque type Thing { inner: Int }

        fn bar(data: Data) -> (Thing, Int, Int) {
          expect x: (Thing, Int, Int) = (data, 1)
          x
        }
    "#;

    assert!(matches!(
        check(parse(from_data)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ));

    assert!(matches!(
        check(parse(from_a_longer_tuple)),
        Err((_, Error::CouldNotUnify { .. }))
    ));
}

#[test]
fn expect_into_opaque_type_in_a_pair_is_an_opaque_cast_only_from_data() {
    let from_data = r#"
        opaque type Thing { inner: Int }

        fn bar(data: Data) -> Pair<Thing, Int> {
          expect x: Pair<Thing, Int> = Pair(data, 1)
          x
        }
    "#;

    let from_an_int = r#"
        opaque type Thing { inner: Int }

        fn bar() -> Pair<Thing, Int> {
          expect x: Pair<Thing, Int> = Pair(1, 1)
          x
        }
    "#;

    assert!(matches!(
        check(parse(from_data)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ));

    assert!(matches!(
        check(parse(from_an_int)),
        Err((_, Error::CouldNotUnify { .. }))
    ));
}

#[test]
fn expect_into_opaque_phantom_type_argument() {
    let phantom = r#"
        opaque type Thing { inner: Int }

        type Tagged<a> {
          Tagged(Int)
        }

        fn bar(data: Data) -> Tagged<Thing> {
          expect x: Tagged<Thing> = data
          x
        }
    "#;

    // The same argument, once a constructor field uses it.
    let used_by_a_field = r#"
        opaque type Thing { inner: Int }

        type Tagged<a> {
          Tagged(Int)
          Holding(List<a>)
        }

        fn bar(data: Data) -> Tagged<Thing> {
          expect x: Tagged<Thing> = data
          x
        }
    "#;

    let result = check(parse(phantom));
    assert!(result.is_ok(), "{result:#?}");

    assert!(matches!(
        check(parse(used_by_a_field)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ));
}

/// `if … is` refuses a type that contains a `String` (or a `Value`, `G1Element`, `G2Element`),
/// which it can't test without failing. A phantom argument never appears in a value, so it
/// doesn't count, as for opaque types; the same argument used by a field still does.
#[test]
fn soft_cast_into_a_phantom_type_argument_that_cant_be_tested() {
    for argument in [
        "String",
        "Value",
        "G1Element",
        "G2Element",
        "Option<String>",
    ] {
        let phantom = format!(
            r#"
            type Tagged<a> {{
              Tagged(Int)
            }}

            fn f(d: Data) -> Bool {{
              if d is x: Tagged<{argument}> {{
                x == x
              }} else {{
                False
              }}
            }}
            "#
        );

        let used_by_a_field = format!(
            r#"
            type Tagged<a> {{
              Tagged(Int)
              Holding(a)
            }}

            fn f(d: Data) -> Bool {{
              if d is x: Tagged<{argument}> {{
                x == x
              }} else {{
                False
              }}
            }}
            "#
        );

        let result = check(parse(&phantom));
        assert!(result.is_ok(), "{argument}: {result:#?}");

        let result = check(parse(&used_by_a_field));
        assert!(
            matches!(result, Err((_, Error::IllegalSoftCast { .. }))),
            "{argument}: {result:#?}"
        );
    }
}

#[test]
fn expect_into_imported_opaque_phantom_type_argument() {
    let dependency = r#"
        pub opaque type Thing { inner: Int }

        pub type Tagged<a> {
          Tagged { amount: Int }
          Untagged
        }
    "#;

    let source_code = r#"
        use thing.{Tagged, Thing}

        fn bar(data: Data) -> Option<Tagged<Thing>> {
          if data is tagged: Tagged<Thing> {
            Some(tagged)
          } else {
            None
          }
        }
    "#;

    let result = check_with_deps(parse(source_code), vec![parse_as(dependency, "thing")]);

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn expect_into_prelude_option_of_opaque_with_local_constructors_named_like_its_own() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        type Maybe<a> {
          Some(Int)
          None
        }

        fn bar(data: Data) -> Option<Thing> {
          expect x: Option<Thing> = data
          x
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ));
}

#[test]
fn expect_into_prelude_option_of_opaque_with_unqualified_imported_constructors_named_like_its_own()
{
    let dependency = r#"
        pub type Maybe<a> {
          Some(Int)
          Nothing
        }
    "#;

    let source_code = r#"
        use maybe.{Some}

        opaque type Thing { inner: Int }

        fn bar(data: Data) -> Option<Thing> {
          expect x: Option<Thing> = data
          x
        }
    "#;

    assert!(matches!(
        check_with_deps(parse(source_code), vec![parse_as(dependency, "maybe")]),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ));
}

#[test]
fn expect_into_prelude_types_of_opaque_with_local_types_named_like_them() {
    let dependency = r#"
        pub opaque type Thing { inner: Int }

        pub type MaybeThing = Option<Thing>

        pub type Things = List<Thing>
    "#;

    let shadowed_option = r#"
        use thing.{MaybeThing}

        type Option<a> {
          Nope
        }

        fn bar(data: Data) -> MaybeThing {
          expect x: MaybeThing = data
          x
        }
    "#;

    let shadowed_list = r#"
        use thing.{Things}

        type List<a> {
          Nil
        }

        fn bar(data: Data) -> Things {
          expect x: Things = data
          x
        }
    "#;

    for source_code in [shadowed_option, shadowed_list] {
        assert!(matches!(
            check_with_deps(parse(source_code), vec![parse_as(dependency, "thing")]),
            Err((_, Error::ExpectOnOpaqueType { .. }))
        ));
    }
}

#[test]
fn a_record_holding_a_prelude_option_of_opaque_is_opaque_despite_a_local_some() {
    let source_code = r#"
        opaque type Thing { inner: Int }

        type Maybe<a> {
          Some(Int)
          None
        }

        type Holder {
          held: Option<Thing>,
        }

        fn bar(data: Data) -> Holder {
          expect x: Holder = data
          x
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::ExpectOnOpaqueType { .. }))
    ));
}

#[test]
fn forbid_a_miller_loop_result_in_a_generic_constructor() {
    let in_a_box = r#"
        use aiken/builtin

        type Box<a> {
          Box(a)
        }

        fn main(g1: G1Element, g2: G2Element) -> Bool {
          let ml = builtin.bls12_381_miller_loop(g1, g2)
          when Box(ml) is {
            Box(inner) -> builtin.bls12_381_final_verify(inner, ml)
          }
        }
    "#;

    let in_an_option = r#"
        use aiken/builtin

        fn main(g1: G1Element, g2: G2Element) -> Bool {
          let ml = builtin.bls12_381_miller_loop(g1, g2)
          when Some(ml) is {
            Some(inner) -> builtin.bls12_381_final_verify(inner, ml)
            None -> False
          }
        }
    "#;

    for source_code in [in_a_box, in_an_option] {
        assert!(
            matches!(
                check(parse(source_code)),
                Err((_, Error::IllegalTypeInData { .. }))
            ),
            "{source_code}"
        );
    }
}

#[test]
fn forbid_a_function_in_a_generic_constructor() {
    let in_a_box = r#"
        type Box<a> {
          Box(a)
        }

        fn main() -> Bool {
          when Box(fn(x) { x + 1 }) is {
            Box(g) -> g(1) == 2
          }
        }
    "#;

    let in_an_option = r#"
        fn main() -> Bool {
          when Some(fn(x) { x + 1 }) is {
            Some(g) -> g(1) == 2
            None -> False
          }
        }
    "#;

    for source_code in [in_a_box, in_an_option] {
        assert!(
            matches!(
                check(parse(source_code)),
                Err((_, Error::IllegalTypeInData { .. }))
            ),
            "{source_code}"
        );
    }
}

#[test]
fn forbid_a_miller_loop_result_stored_by_a_generic_function() {
    let source_code = r#"
        use aiken/builtin

        type Box<a> {
          Box(a)
        }

        fn unbox(b: Box<a>) -> a {
          let Box(x) = b
          x
        }

        fn main(g1: G1Element, g2: G2Element) -> Bool {
          let ml = builtin.bls12_381_miller_loop(g1, g2)
          builtin.bls12_381_final_verify(unbox(Box(ml)), ml)
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ));
}

#[test]
fn forbid_a_function_stored_by_a_generic_function_of_the_same_module() {
    let source_code = r#"
        fn wrap(x: a) -> List<a> {
          [x]
        }

        fn main() -> Int {
          let _ = wrap(fn(n: Int) { n })
          1
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ));
}

#[test]
fn forbid_a_function_stored_by_a_generic_function_of_an_imported_module() {
    let dependency = r#"
        pub fn wrap(x: a) -> List<a> {
          [x]
        }

        pub fn pair_up(x: a, y: b) -> Pair<a, b> {
          Pair(x, y)
        }

        pub fn through(x: a) -> List<a> {
          wrap(x)
        }
    "#;

    let wrap = r#"
        use dep

        fn main() -> Int {
          1
        }

        test t() {
          let _ = dep.wrap(fn(n: Int) { n })
          True
        }
    "#;

    let pair_up = r#"
        use dep

        test t() {
          let _ = dep.pair_up(1, fn(n: Int) { n })
          True
        }
    "#;

    let through = r#"
        use dep

        test t() {
          let _ = dep.through(fn(n: Int) { n })
          True
        }
    "#;

    for source_code in [wrap, pair_up, through] {
        assert!(
            matches!(
                check_with_deps(parse(source_code), vec![parse_as(dependency, "dep")]),
                Err((_, Error::IllegalTypeInData { .. }))
            ),
            "{source_code}"
        );
    }
}

#[test]
fn forbid_a_function_consed_by_the_builtin() {
    let source_code = r#"
        use aiken/builtin

        fn main() -> Int {
          let _xs = builtin.cons_list(fn(n: Int) { n }, [])
          1
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ));
}

#[test]
fn allow_generic_constructors_and_functions_holding_data_forms() {
    let source_code = r#"
        use aiken/builtin

        type Box<a> {
          Box(a)
        }

        type Tagged<a> {
          Tagged(Int)
        }

        fn unbox(b: Box<a>) -> a {
          let Box(x) = b
          x
        }

        fn wrap(x: a) -> List<a> {
          [x]
        }

        fn main(g1: G1Element) -> Bool {
          let tagged: Tagged<Int> = Tagged(1)
          let _ = Some(g1)
          let ys = wrap(Box([1, 2]))
          unbox(Box(1)) == 1 && builtin.length_of_bytearray(#"") == 0 && ys != []
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}

#[test]
fn forbid_a_fuzzer_of_miller_loop_results() {
    let source_code = r#"
        fn int() -> Fuzzer<Int> { todo }

        fn map(fuzzer: Fuzzer<a>, f: fn(a) -> b) -> Fuzzer<b> {
          fn(prng) {
            when fuzzer(prng) is {
              Some((next, a)) -> Some((next, f(a)))
              None -> None
            }
          }
        }

        fn to_ml(_n: Int) -> MillerLoopResult { todo }

        test prop(ml via map(int(), to_ml)) { True }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::IllegalTypeInData { .. }))
    ));
}

#[test]
fn forbid_upcasting_an_empty_list_whose_type_is_a_link_to_an_unknown_type() {
    let through_a_generic_function = r#"
        fn reverse(xs: List<a>) -> List<a> {
          xs
        }

        fn main() -> Data {
          let d: Data = reverse([])
          d
        }
    "#;

    let nested = r#"
        fn reverse(xs: List<a>) -> List<a> {
          xs
        }

        fn main() -> Data {
          let d: Data = Some(reverse([]))
          d
        }
    "#;

    let through_if_branches = r#"
        fn main(c: Bool) -> Data {
          let d: Data =
            if c {
              []
            } else {
              []
            }
          d
        }
    "#;

    let through_when_arms = r#"
        fn main() -> Data {
          let d: Data =
            when 1 is {
              0 -> []
              _ -> []
            }
          d
        }
    "#;

    for source_code in [
        through_a_generic_function,
        nested,
        through_if_branches,
        through_when_arms,
    ] {
        assert!(
            matches!(
                check(parse(source_code)),
                Err((_, Error::AmbiguousEmptyList { .. }))
            ),
            "{source_code}"
        );
    }
}

#[test]
fn forbid_coercing_an_empty_list_whose_type_is_a_link_to_an_unknown_type() {
    let source_code = r#"
        fn main(c: Bool) -> Bool {
          coerce p: Pairs<Int, Int> =
            if c {
              []
            } else {
              []
            }
          p == []
        }
    "#;

    assert!(matches!(
        check(parse(source_code)),
        Err((_, Error::AmbiguousEmptyList { .. }))
    ));
}

#[test]
fn allow_upcasting_a_list_through_a_generic_function_once_its_element_type_is_known() {
    let source_code = r#"
        fn reverse(xs: List<a>) -> List<a> {
          xs
        }

        fn main() -> Data {
          let known: List<Int> = reverse([])
          let d: Data = known
          d
        }

        fn other() -> Data {
          let d: Data = reverse([1])
          d
        }
    "#;

    let result = check(parse(source_code));

    assert!(result.is_ok(), "{result:#?}");
}
