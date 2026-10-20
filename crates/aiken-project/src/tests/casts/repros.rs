//! One reproduction per fixed bug that no other test covers: each program runs optimized
//! and as the baseline, at both trace levels. They are ignored by default: run them with
//! `cargo test -- --ignored`.

use super::assert_tests_pass;

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
