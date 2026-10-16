# Aiken standard library

The standard library ships with the compiler: every Aiken project can use these modules without declaring any dependency, and the library's version is the compiler's version.

- `aiken/*`: general-purpose modules, such as collections (`list`, `dict`, `pairs`), primitives (`bytearray`, `int`, `string`), math, cryptography and CBOR.
- `cardano/*`: types and helpers for transactions, addresses, assets, certificates and governance.
- `aiken/fuzz` and `cardano/fuzz`: fuzzers for property-based tests.
