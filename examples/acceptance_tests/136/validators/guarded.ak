use casts

validator guarded {
  spend(_datum: Option<Data>, redeemer: Data, _own_ref: Data, _self: Data) {
    casts.int(redeemer)
  }

  else(_) {
    fail
  }
}

validator guarded_pair {
  spend(_datum: Option<Data>, redeemer: Data, _own_ref: Data, _self: Data) {
    casts.pair_key(redeemer)
  }

  else(_) {
    fail
  }
}

test validator_accepts_int() {
  guarded.spend(None, 1, 0, 0)
}

test validator_rejects_bytearray() fail {
  guarded.spend(None, #"00", 0, 0)
}

test validator_pair_rejects_bytearray_key() fail {
  guarded_pair.spend(None, [as_data(#"00"), as_data(1)], 0, 0)
}
