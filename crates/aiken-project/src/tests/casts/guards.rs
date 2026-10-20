//! Programs whose result, accepted with these outcomes or refused, every change to casts must
//! keep.

use super::{assert_modules_pass, assert_tests_pass, compile_ok, type_errors};
use aiken_lang::tipo::error::Error as TypeError;
use uplc::{PlutusData, ast::Data};

/// `source` with its top-level definitions in both orders.
fn both_orders(source: &str) -> [String; 2] {
    let mut definitions: Vec<String> = vec![];

    for chunk in source.trim().split("\n\n") {
        match definitions.last_mut() {
            Some(last) if chunk.starts_with(char::is_whitespace) => {
                last.push_str("\n\n");
                last.push_str(chunk);
            }
            _ => definitions.push(chunk.to_string()),
        }
    }

    let forward = definitions.join("\n\n");
    definitions.reverse();

    [forward, definitions.join("\n\n")]
}

#[test]
fn captures_lambdas_and_pipes_that_upcast_their_argument() {
    assert_tests_pass(
        &[(
            "lib/captures.ak",
            r#"
use aiken/builtin
use aiken/cbor
use aiken/collection/list

fn eq_data(d: Data, n: Int) -> Bool {
  d == builtin.i_data(n)
}

fn apply(f: fn(Int) -> Bool) -> Bool {
  f(1)
}

test a_capture_passed_to_list_all() {
  list.all([1, 1], eq_data(_, 1))
}

test a_capture_passed_to_a_local_higher_order_function() {
  apply(eq_data(_, 1))
}

test a_stdlib_function_used_as_a_capture() {
  list.map([1, 2], cbor.serialise(_)) == [#"01", #"02"]
}

test a_let_bound_lambda_whose_argument_type_is_fixed_later() {
  let is_one = fn(x) { eq_data(x, 1) }
  list.all([1, 1], is_one)
}

test an_inline_lambda() {
  list.all([1, 1], fn(x) { eq_data(x, 1) })
}

test a_pipe_into_a_call() {
  1 |> eq_data(_, 1)
}
"#,
        )],
        "captures",
    );
}

#[test]
fn shared_constants_and_mutual_recursion_compile_in_either_order() {
    let programs = [
        (
            "lib/shared.ak",
            "use aiken/cbor\nuse aiken/collection/dict",
            r#"
const no_balances = dict.empty

fn fingerprint() -> ByteArray {
  cbor.serialise(no_balances)
}

fn balance_of(owner: ByteArray) -> Int {
  when dict.get(no_balances, owner) is {
    Some(n) -> n
    None -> 0
  }
}

test empty_registry() {
  balance_of(#"00") == 0 && fingerprint() == #"a0"
}
"#,
        ),
        (
            "lib/shared.ak",
            "use aiken/builtin",
            r#"
const none = None

fn as_raw() -> Data {
  let d: Data = none
  d
}

fn as_int() -> Int {
  when none is {
    Some(n) -> n + 1
    None -> 0
  }
}

test shared_none() {
  as_raw() == builtin.constr_data(1, []) && as_int() == 0
}
"#,
        ),
        (
            "validators/shared.ak",
            "use aiken/cbor",
            r#"
pub type Config {
  owner: Option<ByteArray>,
  fee: Int,
}

const default_config = Config { owner: None, fee: 2 }

fn fingerprint() -> ByteArray {
  cbor.serialise(default_config)
}

validator v {
  mint(_redeemer: Data, policy_id: ByteArray, _self: Data) {
    when default_config.owner is {
      Some(owner) -> owner == policy_id
      None -> fingerprint() != policy_id
    }
  }

  else(_) {
    fail
  }
}

test fingerprint_of_default() {
  fingerprint() == #"d8799fd87a8002ff"
}
"#,
        ),
        (
            "lib/shared.ak",
            "use aiken/builtin",
            r#"
fn walk(x, n: Int) -> Data {
  if n <= 0 {
    let d: Data = x
    d
  } else {
    step(x, n - 1)
  }
}

fn step(y, n: Int) -> Data {
  if y > 0 {
    walk(y, n)
  } else {
    walk(0, n)
  }
}

test mutual() {
  walk(5, 3) == builtin.i_data(5)
}
"#,
        ),
    ];

    for (path, imports, definitions) in programs {
        for definitions in both_orders(definitions) {
            let source = format!("{imports}\n\n{definitions}");
            let compiled = assert_tests_pass(&[(path, &source)], "shared");

            if path.starts_with("validators/") {
                for project in &compiled {
                    project.validator("shared", "v");
                }
            }
        }
    }
}

const SHADOWING_LIB: &str = r#"
pub type Holder {
  h: Option<Int>,
}

pub opaque type Secret {
  inner: Int,
}

pub type Hidden {
  h: Option<Secret>,
}
"#;

#[test]
fn casts_through_generic_and_phantom_types() {
    assert_modules_pass(
        &[
            ("lib/m.ak", SHADOWING_LIB),
            (
                "lib/dep.ak",
                r#"
pub fn same(x: a, y: a) -> Bool {
  x == y
}
"#,
            ),
            (
                "lib/fixed_later.ak",
                r#"
use aiken/builtin
use aiken/collection/list

fn wrap(xs) {
  let n = list.length(xs)
  let d: Data = Some(xs)
  (d, list.foldl(xs, n, fn(x, acc) { x + acc }))
}

fn with_items(k: fn(List<Int>) -> a) -> a {
  k([1, 2])
}

fn backpassed() -> (Data, Int) {
  let xs <- with_items
  let d: Data = Some(xs)
  (d, list.foldl(xs, 0, fn(x, acc) { x + acc }))
}

test wrapped() {
  let (d, total) = wrap([1, 2])
  total == 5 && d == builtin.constr_data(
    0,
    [builtin.list_data([builtin.i_data(1), builtin.i_data(2)])],
  )
}

test backpassed_list() {
  let (d, total) = backpassed()
  total == 3 && d == builtin.constr_data(
    0,
    [builtin.list_data([builtin.i_data(1), builtin.i_data(2)])],
  )
}
"#,
            ),
            (
                "lib/phantom.ak",
                r#"
use aiken/builtin

type Tagged<a> {
  Tagged(Int)
}

type Wrap<a> {
  Wrap(Tagged<a>)
}

test if_is_a_record_with_a_phantom_string_argument() {
  let d: Data = builtin.constr_data(0, [builtin.i_data(1)])
  if d is x: Tagged<String> {
    let Tagged(n) = x
    n == 1
  } else {
    False
  }
}

test if_is_a_nested_record_with_a_phantom_string_argument() {
  let d: Data = builtin.constr_data(0, [builtin.constr_data(0, [builtin.i_data(2)])])
  if d is w: Wrap<String> {
    let Wrap(Tagged(n)) = w
    n == 2
  } else {
    False
  }
}

test if_is_a_phantom_string_argument_takes_the_else_branch_on_a_mismatch() {
  let d: Data = builtin.i_data(1)
  if d is _x: Tagged<String> {
    False
  } else {
    True
  }
}
"#,
            ),
            (
                "lib/generic_eq.ak",
                r#"
use aiken/collection/list.{has}
use aiken/crypto/bls12_381/g1
use dep

fn same(x: a, y: a) -> Bool {
  x == y
}

fn same_again(x: a, y: a) -> Bool {
  same(x, y)
}

test ints() {
  same_again(1, 1)
}

test imported() {
  !dep.same([#"00"], [])
}

test points() {
  let g = g1.generator
  !has([Some(g)], None) && has([Some(g)], Some(g))
}

test data() {
  same(as_data(1), as_data(1))
}
"#,
            ),
            (
                "lib/shadowing.ak",
                r#"
use aiken/builtin
use m

type Option<a> {
  Nope(String)
}

test holder() {
  let d: Data = builtin.constr_data(0, [builtin.constr_data(0, [builtin.i_data(1)])])
  if d is h: m.Holder {
    h.h != None
  } else {
    False
  }
}

test not_a_holder() {
  let d: Data = builtin.i_data(1)
  if d is _h: m.Holder {
    False
  } else {
    True
  }
}

fn nope() -> Option<Int> {
  Nope(@"x")
}

test local_option() {
  when nope() is {
    Nope(s) -> s == @"x"
  }
}
"#,
            ),
        ],
        &["fixed_later", "phantom", "generic_eq", "shadowing"],
    );
}

#[test]
fn a_local_type_named_like_a_prelude_type_doesnt_hide_an_opaque_type() {
    for (module, cast) in [
        ("expect", "expect h: m.Hidden = d\n  h == h"),
        (
            "is",
            "if d is h: m.Hidden {\n    h == h\n  } else {\n    False\n  }",
        ),
    ] {
        let source = format!(
            "use m\n\ntype Option<a> {{\n  Nope(Int)\n}}\n\ntype Local {{\n  Some(Int)\n  None\n}}\n\npub fn f(d: Data) -> Bool {{\n  {cast}\n}}\n"
        );

        let errors = type_errors(&[
            ("lib/m.ak", SHADOWING_LIB),
            (&format!("lib/{module}.ak"), &source),
        ]);

        assert!(
            !errors.is_empty()
                && errors
                    .iter()
                    .all(|error| matches!(error, TypeError::ExpectOnOpaqueType { .. })),
            "{module}: expected an opaque-type error, got {errors:#?}"
        );
    }
}

#[test]
fn expect_of_an_unknown_element_type_fixed_later_checks_the_elements() {
    let compiled = compile_ok(&[(
        "lib/elements.ak",
        r#"
fn ints(data: Data) -> List<Int> {
  expect xs: List<_> = data
  let ys: List<Int> = xs
  ys
}

pub fn total(data: Data) -> Int {
  when ints(data) is {
    [] -> 0
    [x, ..] -> x
  }
}
"#,
    )]);

    let ints = [PlutusData::from(&Data::list(vec![Data::integer(7.into())]))];
    // `total` reads only the head, so a malformed tail fails only if `expect` checks it.
    let bad_tail = [PlutusData::from(&Data::list(vec![
        Data::integer(7.into()),
        Data::bytestring(vec![0xaa]),
    ]))];

    for project in &compiled {
        let total = project.function("elements", "total");

        assert_eq!(
            total.run(&ints),
            super::Outcome::Returned(uplc::ast::Term::integer(7.into()))
        );
        assert_eq!(total.run(&bad_tail), super::Outcome::Failed);
    }
}

/// An erased opaque type can be the only place a function's type parameter appears in its
/// signature: the call still tells which type the function's generics are.
#[test]
fn a_type_parameter_only_in_an_erased_opaque_type_is_resolved() {
    let compiled = assert_tests_pass(
        &[(
            "lib/erased_tag.ak",
            r#"
use aiken/collection/list

pub opaque type Tag<a> {
  Tag(Int)
}

pub fn holds(_tag: Tag<a>, d: Data) -> Bool {
  expect _x: a = d
  True
}

pub fn through(tag: Tag<a>, d: Data) -> Bool {
  holds(tag, d)
}

pub fn holds_int(d: Data) -> Bool {
  let tag: Tag<Int> = Tag(0)
  through(tag, d)
}

pub fn holds_bytes(d: Data) -> Bool {
  let tag: Tag<ByteArray> = Tag(0)
  holds(tag, d)
}

pub fn count(_tag: Tag<a>) -> Int {
  let xs: List<a> = []
  list.length(xs)
}

pub fn roundtrips(_tag: Tag<a>, d: Data) -> Bool {
  expect x: a = d
  as_data(x) == d
}

pub fn compares(_tag: Tag<a>) -> Bool {
  let xs: List<a> = []
  xs == []
}

pub fn roundtrips_both() -> Bool {
  let int_tag: Tag<Int> = Tag(0)
  let bytes_tag: Tag<ByteArray> = Tag(0)
  roundtrips(int_tag, as_data(1)) && roundtrips(bytes_tag, as_data(#"aa"))
}

pub fn compares_both() -> Bool {
  let int_tag: Tag<Int> = Tag(0)
  let bytes_tag: Tag<ByteArray> = Tag(0)
  compares(int_tag) && compares(bytes_tag)
}

test decodes_at_the_phantom_type() {
  holds_int(as_data(1)) && holds_bytes(as_data(#"aa"))
}

test counts_a_list_of_the_phantom_type() {
  let tag: Tag<Int> = Tag(0)
  count(tag) == 0
}

test keys_a_constrained_phantom_type() {
  roundtrips_both() && compares_both()
}
"#,
        )],
        "erased_tag",
    );

    let int = [PlutusData::from(&Data::integer(1.into()))];
    let bytes = [PlutusData::from(&Data::bytestring(vec![0xaa]))];

    for project in &compiled {
        let holds_int = project.function("erased_tag", "holds_int");
        assert_ne!(holds_int.run(&int), super::Outcome::Failed);
        assert_eq!(holds_int.run(&bytes), super::Outcome::Failed);

        let holds_bytes = project.function("erased_tag", "holds_bytes");
        assert_ne!(holds_bytes.run(&bytes), super::Outcome::Failed);
        assert_eq!(holds_bytes.run(&int), super::Outcome::Failed);
    }
}
