//// Every function below expects its input to have a given shape, then turns what it extracted
//// back into `Data` and compares it with the input. Whatever the result is used for, the
//// compiler must keep the check that the input has that shape: the comparison itself is true for
//// any input.

// Plain expects.

pub fn int(d: Data) -> Bool {
  expect n: Int = d
  as_data(n) == d
}

pub fn bytearray(d: Data) -> Bool {
  expect b: ByteArray = d
  as_data(b) == d
}

pub fn list(d: Data) -> Bool {
  expect xs: List<Data> = d
  as_data(xs) == d
}

pub fn pairs(d: Data) -> Bool {
  expect m: Pairs<Data, Data> = d
  as_data(m) == d
}

// Expects that also check every item of a list.

pub fn int_list(d: Data) -> Bool {
  expect xs: List<Int> = d
  as_data(xs) == d
}

pub fn bytearray_pairs(d: Data) -> Bool {
  expect m: Pairs<ByteArray, Int> = d
  as_data(m) == d
}

// Expects on the elements of a pair.

pub fn pair_key(d: Data) -> Bool {
  expect [key, _]: List<Data> = d
  expect Pair(k, _): Pair<Int, Data> = d
  as_data(k) == key
}

pub fn pair_value(d: Data) -> Bool {
  expect [_, value]: List<Data> = d
  expect Pair(_, v): Pair<Data, ByteArray> = d
  as_data(v) == value
}

pub fn pair_list_key(d: Data) -> Bool {
  expect [key, _]: List<Data> = d
  expect Pair(k, _): Pair<List<Data>, Data> = d
  as_data(k) == key
}

// Expects on the elements of a list.

pub fn first_int(d: Data) -> Bool {
  expect [first, _]: List<Data> = d
  expect [n, _]: List<Int> = d
  as_data(n) == first
}

pub fn second_bytearray(d: Data) -> Bool {
  expect [_, second]: List<Data> = d
  expect [_, b]: List<ByteArray> = d
  as_data(b) == second
}

pub fn first_list(d: Data) -> Bool {
  expect [first, _]: List<Data> = d
  expect [xs, _]: List<List<Data>> = d
  as_data(xs) == first
}

pub fn second_pairs(d: Data) -> Bool {
  expect [_, second]: List<Data> = d
  expect [_, m]: List<Pairs<Data, Data>> = d
  as_data(m) == second
}

// Data of every shape, and a pair of each.

const an_int: Data = 1

const a_bytearray: Data = #"01"

const a_list: Data = [1]

const some_pairs: Data = [Pair(1, 2)]

fn two(first: Data, second: Data) -> Data {
  as_data([first, second])
}

// Checked shapes are accepted.

test accepts_int() {
  int(an_int)
}

test accepts_bytearray() {
  bytearray(a_bytearray)
}

test accepts_list() {
  list(a_list)
}

test accepts_pairs() {
  pairs(some_pairs)
}

test accepts_int_list() {
  int_list([1, 2])
}

test accepts_bytearray_pairs() {
  bytearray_pairs([Pair(#"01", 2)])
}

test accepts_pair_key() {
  pair_key(two(an_int, a_bytearray))
}

test accepts_pair_value() {
  pair_value(two(an_int, a_bytearray))
}

test accepts_pair_list_key() {
  pair_list_key(two(a_list, an_int))
}

test accepts_first_int() {
  first_int(two(an_int, an_int))
}

test accepts_second_bytearray() {
  second_bytearray(two(a_bytearray, a_bytearray))
}

test accepts_first_list() {
  first_list(two(a_list, a_list))
}

test accepts_second_pairs() {
  second_pairs(two(some_pairs, some_pairs))
}

// Anything else is rejected.

test rejects_int() fail {
  int(a_bytearray)
}

test rejects_bytearray() fail {
  bytearray(an_int)
}

test rejects_list() fail {
  list(an_int)
}

test rejects_pairs() fail {
  pairs(an_int)
}

test rejects_int_list() fail {
  int_list([as_data(1), as_data(#"00")])
}

test rejects_bytearray_pairs() fail {
  bytearray_pairs([Pair(as_data(#"01"), as_data(2)), Pair(as_data(3), as_data(4))])
}

test rejects_pair_key() fail {
  pair_key(two(a_bytearray, a_bytearray))
}

test rejects_pair_value() fail {
  pair_value(two(an_int, an_int))
}

test rejects_pair_list_key() fail {
  pair_list_key(two(an_int, an_int))
}

test rejects_first_int() fail {
  first_int(two(a_bytearray, an_int))
}

test rejects_second_bytearray() fail {
  second_bytearray(two(a_bytearray, an_int))
}

test rejects_first_list() fail {
  first_list(two(an_int, a_list))
}

test rejects_second_pairs() fail {
  second_pairs(two(some_pairs, an_int))
}
