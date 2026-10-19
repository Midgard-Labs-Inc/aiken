# Code-generation cost baseline

The small cost corpus checks compiled size, execution costs, and results. It runs
in the normal workspace test job. The larger workloads in this directory remain
useful for broader comparisons.

## Run the corpus

```sh
cargo test -p aiken-project codegen_cost_baseline
```

The test calls the compiler and evaluator through library APIs. It does not start
the CLI. Each source case defines a function with runtime arguments. This keeps
constant folding from removing the work that the case measures.

## Read the metrics

The source cases are in `crates/aiken-project/src/tests/codegen_costs.rs`. The
reviewed JSON snapshot is in the adjacent `snapshots` directory.

| Field | Meaning |
| --- | --- |
| `flat_bytes` | The Flat encoding of the compiled function, before inputs are applied. |
| `cbor_bytes` | The same Flat program inside the CBOR byte-string wrapper from `to_cbor`. |
| `cpu_units` | The evaluator's CPU execution units for the named input. |
| `memory_units` | The evaluator's memory execution units for the named input. |
| `result` | The expected integer result. `null` means that evaluation must fail. |
| `cek_slippage` | The number of CEK steps between charges. `1` charges every step. |

The snapshot records the Plutus version, protocol version, cost-model choice, and
UPLC version. Each case runs with silent, compact, and verbose traces. The Git
revision identifies the compiler and the source cases. CPU and memory units are
deterministic execution costs. They are not elapsed time or host memory use.

The corpus uses a local evaluator with `cek_slippage` set to `1`. Each CEK step
is charged immediately. Thus, failure costs include all steps before the error.
No pending steps are lost when evaluation fails.

## Cases

- Large local constants and large module constants have separate cases. Their
  emitted sharing can differ.
- A small local constant protects useful inlining.
- A late record field measures generated list access.
- List-prefix matching checks valid, short, and empty inputs.
- A required `expect` before a branch checks valid and malformed data, including
  a branch that does not use the bound value.

The expected results are checked before the snapshot. A snapshot update cannot
approve a wrong result. Budget exhaustion, machine setup errors, and open terms
cannot satisfy an expected failure. Unavailable protocol features also fail the
test. Case names and input names are part of the snapshot, so their removal also
produces a diff.

## Review a change

Run the test without snapshot updates first. Inspect each changed size or cost.
Explain the change in the pull request. Required validation can increase costs.

After review, update the snapshot:

```sh
INSTA_UPDATE=always cargo test -p aiken-project codegen_cost_baseline
git diff -- crates/aiken-project/src/tests/snapshots
cargo test -p aiken-project codegen_cost_baseline
```

Use the same protocol and trace settings when comparing revisions. Keep a
protocol or cost-model update separate from an optimizer change when possible.
Do not use the input bytes as part of the script-size metric.
