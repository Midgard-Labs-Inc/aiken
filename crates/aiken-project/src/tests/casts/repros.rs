//! One reproduction per fixed bug that no other test covers: each program runs optimized
//! and as the baseline, at both trace levels. They are ignored by default: run them with
//! `cargo test -- --ignored`.

use super::{Outcome, assert_tests_pass};
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
