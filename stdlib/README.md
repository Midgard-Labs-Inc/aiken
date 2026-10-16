# Aiken standard library

The standard library ships with the compiler: every Aiken project can use these modules without declaring any dependency, and the library's version is the compiler's version.

It's an ordinary Aiken project. Tests live next to the code in `*.test.ak` modules, which don't ship with the compiler. To type-check the library and run its tests:

```sh
cargo test -p aiken-project --test stdlib
```

Changes go in the repository's [CHANGELOG](../CHANGELOG.md), prefixed with **stdlib**.
