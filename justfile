#!/usr/bin/env -S just --justfile

_default:
  just --list -u

alias r := ready
alias c := codecov
alias t := test

# Initialize the project by installing all the necessary tools.
# Make sure you have cargo-binstall installed.
# You can download the pre-compiled binary from <https://github.com/cargo-bins/cargo-binstall#installation>
# or install via `cargo install cargo-binstall`
init:
  cargo binstall cargo-nextest cargo-watch cargo-insta typos-cli taplo-cli wasm-pack cargo-llvm-cov -y

# When ready, run the same CI commands
ready:
  typos
  cargo fmt
  just check
  just test
  just lint
  git status

# Update our local branch with the remote branch (this is for you to sync the submodules)
update:
  git pull
  git submodule update --init

# Install necessary dependencies.
# `pacquet/tasks/registry-mock` is a member of the root pnpm workspace,
# so the root install populates its node_modules.
install:
  pnpm install --frozen-lockfile --prefer-offline

# Run `cargo watch`
# --no-vcs-ignores: cargo-watch has a bug loading all .gitignores, including the ones listed in .gitignore
# use .ignore file getting the ignore list
watch command:
  cargo watch --no-vcs-ignores -x '{{command}}'

# Format all files
fmt:
  cargo fmt
  taplo format

# Run cargo check
check:
  cargo check --locked --workspace --all-targets

# Run all the tests.
test:
  cargo nextest run

# Run pacquet package tests only.
test-pacquet:
  cargo nextest run --workspace --exclude pnpr --exclude pnpr-fixtures

# Run pnpr package tests only.
test-pnpr:
  cargo nextest run -p pnpr -p pnpr-fixtures

# List expected-failing test ports
[unix]
known-failures:
  @cargo test --workspace known_failures -- --list 2>/dev/null | rg '^known_failures::'

[windows]
known-failures:
  @cargo test --workspace known_failures -- --list 2>nul | rg '^known_failures::'
# Lint the whole project
lint:
  cargo clippy --locked --workspace --all-targets -- --deny warnings

# Run perfectionist dylint rules. Requires `cargo-dylint` and `dylint-link`
# (install with `cargo binstall cargo-dylint dylint-link`). The lint library
# is pinned in `dylint.toml`.
dylint:
  env RUSTFLAGS="-D warnings" cargo dylint --all -- --all-targets --workspace

# Get code coverage
codecov:
  cargo codecov --html

# Run the benchmarks. See `tasks/benchmark`
micro-benchmark:
  cargo run --bin=micro-benchmark --release

# Manage registry-mock. The launcher spawns `pnpr`; on
# Windows you can't overwrite a running .exe, so we pre-build all
# the test artifacts a subsequent `just test` will need with the
# exact same invocation. A `-p pnpr`-scoped pre-build is
# not enough — workspace-wide feature unification gives a
# different fingerprint and nextest would still try to re-link the
# running binary, failing with `os error 5` on Windows MSVC.
registry-mock +args:
  cargo nextest run --no-run
  cargo run --bin=pacquet-registry-mock -- {{args}}

# The benchmark may auto-spawn the registry mock (via
# `AutoMockInstance::load_or_init()`), so make sure `pnpr`
# is built before the executor runs — otherwise the spawn step
# aborts with "binary not found". Built with `--release` so the
# mock serves at optimized perf; a debug build would put the
# Rust mock at a multi-second handicap vs verdaccio, which V8
# always JITs, polluting the install-perf signal.
integrated-benchmark +args:
  cargo build --release --bin=pnpr
  cargo run --bin=integrated-benchmark -- {{args}}

cli +args:
  cargo run --bin pacquet -- {{args}}
