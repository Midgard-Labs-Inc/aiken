# AGENTS.md

This file provides guidance to AI coding agents working with code in this repository.

Aiken is a smart contract language and toolchain for Cardano. This repository is a fork of `aiken-lang/aiken`; its crates are not published to crates.io (`publish = false`), and the only shipped artifact is the `aiken` binary, released with dist.

## Commands

The toolchain is pinned in `rust-toolchain.toml`; rustup installs it automatically. `nix develop` provides a dev shell built from the same file (plus `cargo-insta`).

CI (`.github/workflows/tests.yml`) runs:

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

Narrower loops:

```sh
cargo test -p aiken-lang <test_name>        # parser, type checker, formatter
cargo test -p aiken-project <test_name>     # codegen (src/tests/gen_uplc.rs), blueprints
cargo test -p uplc <test_name>              # CEK machine, flat, tx evaluation
cargo test -p uplc --test conformance       # Plutus conformance suite (test_data/conformance)
cargo test -p aiken-project --test stdlib   # type-check the standard library and run its tests
cargo insta review                          # accept/reject changed snapshots
cargo run -- check path/to/project          # run the CLI against an Aiken project
```

Many tests are `insta` snapshot tests (`snapshots/` directories next to the code). Parser and formatter tests use the `assert_expr!`, `assert_definition!`, `assert_format!`, etc. macros from `crates/aiken-lang/src/parser/utils.rs`.

End-to-end checks on Aiken projects (CI runs these too):

```sh
cargo run -r -- check examples/hello_world   # also gift_card, monorepo
cargo run -r -- check benchmarks
cd examples/acceptance_tests && ./run 034    # one scenario; `bash ci` runs all
```

`examples/acceptance_tests/script_context` tests need `jq` and `cbor-diag` (`cargo install cbor-diag-cli`).

## Docs site

`site/` is the documentation website, built with Astro and Cloudflare's Nimbus and managed with pnpm. `site/AGENT.md` is the guidance for working in it.

```sh
cd site
pnpm install
pnpm dev          # dev server with hot reload
pnpm build        # static build to site/dist
pnpm typecheck
pnpm lint:docs
```

`site/src/content/docs/stdlib/` is generated from the standard library's doc comments (with `aiken docs --format markdown`'s renderer) and must not be hand-edited. A test fails when it's out of date; after changing stdlib docs, regenerate it with:

```sh
UPDATE_STDLIB_DOCS=1 cargo test -p aiken-project --test stdlib
```

Stdlib doc comments are rendered as MDX: use Nimbus admonitions (`:::note … :::`) rather than GitHub-style alerts, and close every code fence.

`site/src/content/docs/prelude/` documents the prelude (`aiken`) and the builtins (`aiken/builtin`), which are defined by the compiler (`crates/aiken-lang/src/builtins.rs`, and `crates/uplc/src/builtins.rs` for the builtins). These pages are maintained by hand: when you add, remove or change a prelude type or function, or a builtin, update them in the same change.

`site/src/content/docs/language-tour/` is the language tour, also maintained by hand: document new syntax or language features there in the same change. Compiler diagnostics link to its sections (`#[diagnostic(url("https://aiken-lang.com/language-tour/..."))]`), so when you rename or move a heading, update the links that point at its anchor.

## Architecture

`stdlib/` is the standard library (including the fuzzers). It's an ordinary Aiken project, but it ships inside the compiler: `crates/aiken-project/build.rs` embeds its modules (minus the `*.test.ak` test modules), and every project gets them written to `build/packages/aiken-stdlib` and loaded as an implicit dependency named `aiken/stdlib` (`crates/aiken-project/src/stdlib.rs`). Its version is the compiler's version. Projects can't declare it as a dependency, and their own modules can't live under the `aiken/*` or `cardano/*` namespaces.

Crates in `crates/`, from the CLI down:

- **`aiken`**: the CLI (`src/cmd/*`, one module per subcommand: `new`, `check`, `build`, `fmt`, `docs`, `blueprint`, `tx`, `uplc`, `lsp`, ...). Thin wrappers that build an `aiken_project::Project` and run it.
- **`aiken-lsp`**: language server, built on `aiken-project`.
- **`aiken-project`**: everything at the project level. `Project` (`src/lib.rs`) reads `aiken.toml`, fetches dependencies from GitHub into `build/packages` (`deps/`, `github/`), and drives compilation. `Project::compile` is the pipeline: `read_source_files` → `parse_sources` → `type_check`, then either builds the CIP-57 blueprint (`blueprint/`, written to `plutus.json`) or collects and runs tests and benchmarks. Also hosts docs generation (`docs/`) and `export`.
- **`aiken-lang`**: the language itself. Lexer and chumsky-based parser (`parser/`) produce the untyped AST (`ast.rs`); `tipo/` does type inference and exhaustiveness checking, producing the typed AST; `format.rs` is the formatter. `gen_uplc.rs` (`CodeGenerator`) compiles typed code to UPLC: it lowers expressions to an intermediate AIR tree (`gen_uplc/tree.rs`, `air.rs`), hoists shared functions, compiles pattern matching through decision trees (`gen_uplc/decision_tree.rs`), then emits UPLC terms. `test_framework.rs` defines how Aiken `test`/`bench` definitions (unit and property-based) are compiled and evaluated.
- **`uplc`**: Untyped Plutus Core. AST, parser and pretty-printer, flat encoding, DeBruijn conversion, the optimizer (`optimize/shrinker.rs`), and the CEK machine (`machine/`, with cost models). `tx/` does phase-one and phase-two transaction evaluation, including building the script context for each Plutus version.

Validators and tests both end up as UPLC programs: `CodeGenerator::generate` for validators, `generate_raw` for tests/benchmarks, which `aiken-project` then runs on the `uplc` machine.

## Conventions

- **NEVER add yourself as a co-author.** No `Co-Authored-By` trailers, "Generated with" lines, or any other agent attribution in commit messages or PR descriptions.
- **NEVER write tests that invoke the CLI as a binary** (no `std::process::Command`, `assert_cmd`, `cargo run`, or shell scripts calling `aiken`). Test the functions the CLI calls instead: `aiken_project::Project` methods, `aiken_lang` / `uplc` functions, etc. If something is only reachable through `crates/aiken/src/cmd`, move the logic into a library crate and test it there.
- Add a `CHANGELOG.md` entry under `## unreleased`, in the `Added` / `Changed` / `Fixed` / `Removed` subsection, prefixed with the crate in bold, e.g. `- **uplc**: ...` (or `- **stdlib**: ...` for the standard library).
- Release config lives in `dist-workspace.toml`. `.github/workflows/release.yml` is generated by `dist generate` and must not be hand-edited; dist fails CI if it is out of date.
