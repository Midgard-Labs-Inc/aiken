//// Casts through Data: what each cast accepts and rejects at runtime.

use aiken/builtin

// ## Value, G1Element and G2Element convert to and from Data

test value_upcast_then_expect_roundtrips() {
  let v: Value = { lovelace: 10 }
  let d: Data = v
  expect w: Value = d
  w == v
}

test value_upcast_compares_as_data() {
  let v: Value = { lovelace: 10 }
  let d: Data = builtin.value_data(v)
  d == v
}

test value_expect_rejects_an_integer() fail {
  expect w: Value = builtin.i_data(1)
  w == w
}

test value_expect_rejects_a_zero_quantity() fail {
  let zero =
    builtin.map_data(
      [
        Pair(
          builtin.b_data(#""),
          builtin.map_data([Pair(builtin.b_data(#""), builtin.i_data(0))]),
        ),
      ],
    )
  expect w: Value = zero
  w == w
}

test g1_upcast_then_expect_roundtrips() {
  let p = builtin.bls12_381_g1_hash_to_group("abc", "dst")
  let d: Data = p
  expect q: G1Element = d
  builtin.bls12_381_g1_equal(p, q)
}

test g1_expect_rejects_invalid_bytes() fail {
  expect q: G1Element = builtin.b_data(#"00")
  builtin.bls12_381_g1_equal(q, q)
}

test g2_upcast_then_expect_roundtrips() {
  let p = builtin.bls12_381_g2_hash_to_group("abc", "dst")
  let d: Data = p
  expect q: G2Element = d
  builtin.bls12_381_g2_equal(p, q)
}

test record_with_value_expect_roundtrips() {
  let wrapped = Wrapped { amount: 1, value: { lovelace: 42 } }
  let d: Data = wrapped
  expect w: Wrapped = d
  w == wrapped
}

pub type Wrapped {
  amount: Int,
  value: Value,
}

// ## Opaque wrappers cast like the type they wrap

opaque type Lovelace {
  Lovelace(Value)
}

test opaque_value_wrapper_coerce_roundtrips() {
  let v: Value = { lovelace: 7 }
  coerce l: Lovelace = builtin.value_data(v)
  let d: Data = l
  d == v
}

// ## Pipes upcast their argument like a call

fn data_is_int(d: Data) -> Bool {
  if d is Int {
    True
  } else {
    False
  }
}

test pipe_upcasts_its_argument() {
  42 |> data_is_int
}

// ## A cast meeting an unknown type links it to Data

test unknown_type_becomes_data() {
  let to_data =
    fn(x) {
      let d: Data = x
      d
    }
  to_data(builtin.i_data(1)) == builtin.i_data(1)
}

// ## Opaque checks see through type links and phantom arguments

opaque type Thing {
  inner: Int,
}

fn id(x: a) -> a {
  x
}

test expect_on_an_opaque_value_through_a_link() {
  let t = Thing { inner: 1 }
  expect x: Thing = id(t)
  x.inner == 1
}

type Tagged<a> {
  Tagged(Int)
}

test expect_into_a_phantom_opaque_argument() {
  expect Tagged(n): Tagged<Thing> = builtin.constr_data(0, [builtin.i_data(5)])
  n == 5
}

test expect_into_a_phantom_opaque_argument_rejects_bad_data() fail {
  expect Tagged(n): Tagged<Thing> = builtin.constr_data(1, [])
  n == 5
}

// ## coerce doesn't check the constructor index

type Three {
  One
  Two
  Three
}

test coerce_out_of_range_index_takes_the_last_clause() {
  coerce t: Three = builtin.constr_data(7, [])
  when t is {
    One -> False
    Two -> False
    Three -> True
  }
}

test coerce_out_of_range_index_takes_the_wildcard() {
  coerce t: Three = builtin.constr_data(7, [])
  when t is {
    One -> False
    _ -> True
  }
}

test expect_rejects_an_out_of_range_index() fail {
  expect t: Three = builtin.constr_data(7, [])
  t == One
}

// ## `==` against Data compares both operands as Data, in either order

fn as_raw(d: Data) -> Data {
  d
}

test int_equals_data_in_both_orders() {
  let n = 1
  let d = as_raw(builtin.i_data(1))
  n == d && d == n && n != as_raw(builtin.i_data(2))
}

test bytearray_equals_data_in_both_orders() {
  let b = #"01"
  let d = as_raw(builtin.b_data(#"01"))
  b == d && d == b
}

test bool_not_equals_data() {
  let b = True
  b != as_raw(builtin.constr_data(0, [])) && b == as_raw(
    builtin.constr_data(1, []),
  )
}

test g1_equals_data_in_both_orders() {
  let p = builtin.bls12_381_g1_hash_to_group("abc", "dst")
  let d = as_raw(builtin.b_data(builtin.bls12_381_g1_compress(p)))
  p == d && d == p
}

test g2_equals_data_in_both_orders() {
  let p = builtin.bls12_381_g2_hash_to_group("abc", "dst")
  let d = as_raw(builtin.b_data(builtin.bls12_381_g2_compress(p)))
  p == d && d == p
}

test value_equals_data_in_both_orders() {
  let v: Value = { lovelace: 10 }
  let d = as_raw(builtin.value_data(v))
  v == d && d == v
}
