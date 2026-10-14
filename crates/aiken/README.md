# Aiken

A modern smart contract platform for Cardano: a language, compiler, and toolchain for writing and testing on-chain validators.

Documentation: [aiken-lang.com](https://aiken-lang.com)

## Install

**macOS / Linux** (installs to `~/.aiken/bin`):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Midgard-Labs-Inc/aiken/releases/latest/download/aiken-installer.sh | sh
```

**Windows** (PowerShell):

```sh
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Midgard-Labs-Inc/aiken/releases/latest/download/aiken-installer.ps1 | iex"
```

**Homebrew**:

```sh
brew install Midgard-Labs-Inc/tap/aiken
```

**npm**:

```sh
npm install -g @orbistry/aiken
```

Prebuilt archives for every platform are also attached to each [GitHub release](https://github.com/Midgard-Labs-Inc/aiken/releases).

## Getting started

```sh
aiken new my-org/my-project
cd my-project
aiken check
aiken build
```

`aiken check` type-checks the project and runs its tests; `aiken build` compiles the validators into a `plutus.json` blueprint.

## Community

- [Discord](https://discord.gg/TTCjYCtYf9)
- [X (@aiken_dev)](https://x.com/aiken_dev)
- [GitHub](https://github.com/Midgard-Labs-Inc/aiken)
