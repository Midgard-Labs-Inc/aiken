//! One reproduction per fixed bug that no other test covers: each program runs optimized
//! and as the baseline, at both trace levels. They are ignored by default: run them with
//! `cargo test -- --ignored`.

use super::{Outcome, assert_tests_pass, compile_ok};
use aiken_lang::gen_uplc::Error as CodeGenError;
use uplc::{
    PlutusData,
    ast::{Data, Term},
};

fn data(data: Data) -> PlutusData {
    PlutusData::from(&data)
}

#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn a_record_update_keeps_the_tag() {
    assert_tests_pass(
        &[(
            "lib/update_tag.ak",
            r#"
use aiken/builtin

@tag(5)
pub type State {
  count: Int,
  owner: ByteArray,
}

pub type Ctor {
  @tag(6)
  Ctor { n: Int }
}

test type_tag() {
  let s = State { count: 1, owner: #"aa" }
  as_data(State { ..s, count: 2 }) == builtin.constr_data(
    5,
    [builtin.i_data(2), builtin.b_data(#"aa")],
  )
}

test constructor_tag() {
  let c = Ctor { n: 1 }
  as_data(Ctor { ..c, n: 3 }) == builtin.constr_data(6, [builtin.i_data(3)])
}
"#,
        )],
        "update_tag",
    );
}

#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn an_opaque_type_with_a_decorator_is_not_erased() {
    assert_tests_pass(
        &[(
            "lib/decorated.ak",
            r#"
use aiken/builtin

pub opaque type W {
  @tag(3)
  W(Int)
}

fn mk(n: Int) -> W {
  W(n)
}

fn get(w: W) -> Int {
  let W(n) = w
  n
}

@tag(5)
pub opaque type V {
  inner: Int,
}

fn mkv(n: Int) -> V {
  V { inner: n }
}

test documented_upcast() {
  as_data(mk(5)) == builtin.constr_data(3, [builtin.i_data(5)])
}

test documented_coerce() {
  coerce w: W = builtin.constr_data(3, [builtin.i_data(5)])
  get(w) == 5
}

test nested_pattern() {
  when Some(mkv(4)) is {
    Some(V { inner }) -> inner == 4
    None -> False
  }
}

test nested_literal_pattern() {
  when Some(mkv(4)) is {
    Some(V { inner: 4 }) -> True
    _ -> False
  }
}
"#,
        )],
        "decorated",
    );
}

const ERASED: &str = r#"
use aiken/builtin
use aiken/collection/list

pub opaque type Amount {
  Amount(Int)
}

pub opaque type Payload {
  Payload(Data)
}

pub opaque type Items {
  Items(List<Int>)
}

pub opaque type Both {
  Both(Pair<Int, ByteArray>)
}

pub opaque type Triple {
  Triple((Int, Int, Int))
}

pub opaque type Raw {
  Raw(Data)
}

pub opaque type W {
  inner: Int,
}

pub opaque type WD {
  inner: Data,
}

pub type Holder {
  p: Payload,
  a: Amount,
}

fn amount(a: Amount) -> Int {
  let Amount(n) = a
  n
}

fn unwrap(p: Payload) -> Data {
  let Payload(d) = p
  d
}

fn checked(d: Data) -> W {
  expect n: Int = d
  W { inner: n }
}

pub fn spread(d: Data) -> Int {
  let w = W { ..checked(d), inner: 1 }
  w.inner
}

fn apply(f: fn(Int) -> Amount, x: Int) -> Amount {
  f(x)
}

fn literal(a: Amount) -> Int {
  when a is {
    Amount(5) -> 1
    _ -> 0
  }
}

fn list_pattern(i: Items) -> Int {
  when i is {
    Items([]) -> 0
    Items([x]) -> x
    _ -> -1
  }
}

const constant_amount: Amount = Amount(5)

const constant_payload: Payload = Payload(5)

const constant_holder: Holder = Holder { p: Payload(7), a: Amount(8) }

test if_is_matches_every_field_type() {
  let int = if Amount(6) is Amount(n): Amount { n == 6 } else { False }
  let list = if Items([1, 2]) is Items(xs): Items { xs == [1, 2] } else { False }
  let pair = if Both(Pair(1, #"aa")) is Both(p): Both { p.2nd == #"aa" } else { False }
  let tuple = if Triple((1, 2, 3)) is Triple(t): Triple { t.3rd == 3 } else { False }
  let data = if Raw(as_data(1)) is Raw(d): Raw { d == builtin.i_data(1) } else { False }
  int && list && pair && tuple && data
}

test the_constructor_upcasts_its_argument() {
  unwrap(Payload(5)) == builtin.i_data(5) && unwrap(Payload([1, 2])) == builtin.list_data(
    [builtin.i_data(1), builtin.i_data(2)],
  )
}

test a_record_update_gives_the_new_field() {
  let w = W { inner: 1 }
  let wd = WD { inner: builtin.i_data(1) }
  W { ..w, inner: 3 }.inner == 3 && WD { ..wd, inner: Amount(5) }.inner == builtin.i_data(5)
}

test the_constructor_is_the_identity() {
  let f = Amount
  amount(apply(Amount, 5)) == 5 && amount(f(6)) == 6 && list.map([1, 2], Amount) == [
    Amount(1),
    Amount(2),
  ]
}

test patterns_inside_the_constructor_test_the_field() {
  let nested =
    when Some(Amount(5)) is {
      Some(Amount(5)) -> True
      _ -> False
    }
  literal(Amount(5)) == 1 && literal(Amount(4)) == 0 && nested && list_pattern(Items([])) == 0 && list_pattern(
    Items([3]),
  ) == 3
}

test constants_hold_the_erased_forms() {
  amount(constant_amount) == 5 && unwrap(constant_payload) == builtin.i_data(5) && as_data(
    constant_holder,
  ) == builtin.constr_data(0, [builtin.i_data(7), builtin.i_data(8)])
}
"#;

#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn erased_opaque_types_are_newtypes() {
    let compiled = assert_tests_pass(&[("lib/erased.ak", ERASED)], "erased");

    for project in &compiled {
        let spread = project.function("erased", "spread");

        assert_eq!(
            spread.run(&[data(Data::integer(2.into()))]),
            Outcome::Returned(Term::integer(1.into()))
        );
        // The spread's check still runs.
        assert_eq!(
            spread.run(&[data(Data::bytestring(vec![0xaa]))]),
            Outcome::Failed
        );
    }
}

#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn equality_with_data_compares_as_data() {
    assert_tests_pass(
        &[(
            "lib/equality.ak",
            r#"
use aiken/builtin

test int_eq() {
  let d: Data = builtin.i_data(1)
  1 == d
}

test int_neq() {
  let d: Data = builtin.i_data(2)
  1 != d
}

test bool_neq() {
  let d: Data = builtin.constr_data(1, [])
  False != d
}

test data_neq_constant() {
  let d: Data = builtin.constr_data(0, [])
  d != True
}

test bool_eq() {
  let d: Data = builtin.constr_data(1, [])
  True == d
}

test bytes_eq() {
  let d: Data = builtin.b_data(#"aa")
  #"aa" == d
}

test data_eq_int() {
  let d: Data = builtin.i_data(1)
  d == 1 && !(d != 1) && d != 2
}
"#,
        )],
        "equality",
    );
}

#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn upcasts_convert_at_every_position() {
    assert_tests_pass(
        &[(
            "lib/up.ak",
            r#"
use aiken/builtin

type Box {
  d: Data,
  n: Int,
}

fn eq_data(d: Data, n: Int) -> Bool {
  d == builtin.i_data(n)
}

fn all(xs: List<a>, f: fn(a) -> Bool) -> Bool {
  when xs is {
    [] -> True
    [x, ..rest] -> f(x) && all(rest, f)
  }
}

test call_argument() {
  eq_data(1, 1)
}

test record_field() {
  Box { d: 1, n: 0 }.d == builtin.i_data(1)
}

test record_update() {
  let b = Box { d: 1, n: 0 }
  Box { ..b, d: 2 }.d == builtin.i_data(2)
}

test let_annotation() {
  let d: Data = 1
  d == builtin.i_data(1)
}

test pipe() {
  1 |> eq_data(1)
}

test capture() {
  all([1, 1], eq_data(_, 1)) && !all([1, 2], eq_data(_, 1))
}

// The annotation makes `one` a `Data`, not an `Int`.
const one: Data = 1

test constant_annotation() {
  one == builtin.i_data(1)
}

test trace_argument() {
  trace @"label": 1
  True
}
"#,
        )],
        "up",
    );
}

#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn an_element_type_that_nothing_fixes_is_data() {
    let compiled = assert_tests_pass(
        &[(
            "lib/unfixed.ak",
            r#"
use aiken/builtin
use aiken/collection/list

type Box<a> {
  items: List<a>,
}

fn flag() -> Bool {
  True
}

fn n() -> Int {
  0
}

fn with_items(k: fn(List<a>) -> b) -> b {
  k([])
}

pub fn coerce_pairs() -> Pairs<Int, Int> {
  coerce p: Pairs<Int, Int> = if flag() { [] } else { [] }
  p
}

test bare() {
  let d: Data = []
  d == builtin.list_data([])
}

test in_an_option() {
  let a: Data = Some([])
  a == builtin.constr_data(0, [builtin.list_data([])])
}

test in_a_tuple() {
  let b: Data = ([], 1)
  b == builtin.list_data([builtin.list_data([]), builtin.i_data(1)])
}

test in_a_pair() {
  let c: Data = Pair(1, [])
  c == builtin.list_data([builtin.i_data(1), builtin.list_data([])])
}

test in_a_list() {
  let d: Data = [[]]
  d == builtin.list_data([builtin.list_data([])])
}

test through_a_generic_function() {
  let e: Data = list.reverse([])
  e == builtin.list_data([])
}

test in_branches() {
  let f: Data = if flag() { [] } else { [] }
  let g: Data = when n() is {
    0 -> []
    _ -> []
  }
  f == builtin.list_data([]) && g == builtin.list_data([])
}

test in_a_record() {
  let d: Data = Box([])
  d == builtin.constr_data(0, [builtin.list_data([])])
}

test fixed_later_in_the_definition() {
  let xs <- with_items
  let d: Data = xs
  let total = list.foldl(xs, 0, fn(x, acc) { x + acc })
  d == builtin.list_data([]) && total == 0
}
"#,
        )],
        "unfixed",
    );

    // `coerce` checks nothing: a `List []` read as `Pairs` fails.
    for project in &compiled {
        assert_eq!(
            project.function("unfixed", "coerce_pairs").run(&[]),
            Outcome::Failed
        );
    }
}

#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn a_hole_that_nothing_fixes_decodes_data() {
    let compiled = assert_tests_pass(
        &[(
            "lib/hole.ak",
            r#"
use aiken/builtin

pub fn first(d: Data) -> Data {
  expect Some(x): Option<_> = d
  x
}

test hole() {
  let d: Data = builtin.constr_data(0, [builtin.i_data(1)])
  expect x: Option<_> = d
  x != None
}
"#,
        )],
        "hole",
    );

    for project in &compiled {
        let first = project.function("hole", "first");

        assert_ne!(
            first.run(&[data(Data::constr(0, vec![Data::bytestring(vec![0xaa])]))]),
            Outcome::Failed
        );
        assert_eq!(first.run(&[data(Data::constr(0, vec![]))]), Outcome::Failed);
    }
}

const GENERIC_DECODER: &str = r#"
use aiken/builtin

pub type A {
  A(Int)
}

pub type B {
  B(ByteArray)
}

pub fn decode(d: Data) -> a {
  expect x: a = d
  x
}

pub fn wrap(d: Data) -> a {
  decode(d)
}

pub fn decode_a(d: Data) -> A {
  decode(d)
}

pub fn wrap_b(d: Data) -> B {
  wrap(d)
}

test two_records() {
  let a: A = decode(builtin.constr_data(0, [builtin.i_data(1)]))
  let b: B = decode(builtin.constr_data(0, [builtin.b_data(#"aa")]))
  a == A(1) && b == B(#"aa")
}

test two_records_through_a_wrapper() {
  let a: A = wrap(builtin.constr_data(0, [builtin.i_data(1)]))
  let b: B = wrap(builtin.constr_data(0, [builtin.b_data(#"aa")]))
  a == A(1) && b == B(#"aa")
}
"#;

/// A decode at a type parameter converts to the type each call picks, so two record types of
/// the same runtime shape need two variants.
#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn a_generic_decoder_has_a_variant_per_type() {
    let compiled = assert_tests_pass(&[("lib/decoder.ak", GENERIC_DECODER)], "decoder");

    let int_field = [data(Data::constr(0, vec![Data::integer(1.into())]))];
    let bytes_field = [data(Data::constr(0, vec![Data::bytestring(vec![0xaa])]))];

    for project in &compiled {
        let decode_a = project.function("decoder", "decode_a");
        assert_ne!(decode_a.run(&int_field), Outcome::Failed);
        assert_eq!(decode_a.run(&bytes_field), Outcome::Failed);

        let wrap_b = project.function("decoder", "wrap_b");
        assert_ne!(wrap_b.run(&bytes_field), Outcome::Failed);
        assert_eq!(wrap_b.run(&int_field), Outcome::Failed);
    }
}

const POLYMORPHIC_RECURSION: &str = r#"
pub type Tagged<a> {
  Tagged(Int)
}

pub fn f(x: a, n: Int) -> Int {
  if n <= 0 {
    0
  } else {
    g(Some(x), n - 1)
  }
}

pub fn g(x: b, n: Int) -> Int {
  f(x, n - 1)
}

test recurses() {
  f(1, 4) == 0
}
"#;

/// Polymorphic recursion reaches `f` at `Option<Option<…>>`. Without a decode, its variants
/// are keyed by shape, so there are finitely many.
#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn polymorphic_recursion_without_a_decode_compiles() {
    assert_tests_pass(&[("lib/recursion.ak", POLYMORPHIC_RECURSION)], "recursion");
}

/// Compiles `source`'s test and checks that it's refused with `InfiniteVariants` on `f` or `g`.
fn assert_infinite_variants(source: &str) {
    for project in compile_ok(&[("lib/recursion.ak", source)]) {
        let (_, program) = project.test_programs("recursion").remove(0);
        let error = program.unwrap_err();

        assert!(
            matches!(&error, CodeGenError::InfiniteVariants { function, .. } if function == "recursion.f" || function == "recursion.g"),
            "{error:#?}"
        );
    }
}

const DECODE_X: (&str, &str) = (
    "  if n <= 0 {\n    0\n",
    "  if n <= 0 {\n    let d: Data = x\n    expect _y: a = d\n    0\n",
);

/// With a decode of `x`, every type `f` is reached at needs its own variant, and there is no end
/// to them.
#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn polymorphic_recursion_with_a_decode_is_refused() {
    assert_infinite_variants(&POLYMORPHIC_RECURSION.replace(DECODE_X.0, DECODE_X.1));
}

/// Through `(x, x)`, the type doubles at each step: it's refused before its key or its decoder
/// grows with it.
#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn polymorphic_recursion_with_a_decode_of_a_doubling_type_is_refused() {
    assert_infinite_variants(
        &POLYMORPHIC_RECURSION
            .replace(DECODE_X.0, DECODE_X.1)
            .replace("g(Some(x), n - 1)", "g((x, x), n - 1)"),
    );
}

/// Only `x` is decoded, but each variant keyed by `x`'s full type rebuilds `y`, keyed by its
/// shape, at twice the size: the type-size bound refuses the recursion long before the depth
/// bound would.
#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn polymorphic_recursion_with_a_decode_and_a_doubling_argument_is_refused() {
    assert_infinite_variants(
        r#"
pub fn f(x: a, y: b, n: Int) -> Int {
  if n <= 0 {
    let d: Data = x
    expect _x: a = d
    0
  } else {
    g(Some(x), (y, y), n - 1)
  }
}

pub fn g(x: a, y: b, n: Int) -> Int {
  f(x, y, n - 1)
}

test recurses() {
  f(1, 2, 4) == 0
}
"#,
    );
}

/// A decode that mentions the parameter only in a phantom position needs no variant per type.
#[test]
#[ignore = "reproduction: run with `cargo test -- --ignored`"]
fn polymorphic_recursion_with_a_phantom_decode_compiles() {
    let source = POLYMORPHIC_RECURSION.replace(
        "  if n <= 0 {\n    0\n",
        "  if n <= 0 {\n    let d: Data = Tagged(1)\n    expect _t: Tagged<a> = d\n    0\n",
    );

    assert_tests_pass(&[("lib/recursion.ak", &source)], "recursion");
}
