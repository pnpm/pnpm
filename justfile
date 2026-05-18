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
  just install-hooks

# Point git at pacquet/.githooks/ so the tracked pre-push format check runs on `git push`.
install-hooks:
  git config core.hooksPath pacquet/.githooks

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
  cargo check --locked

# Run all the tests
test:
  cargo nextest run

# List expected-failing test ports
[unix]
known-failures:
  @cargo test --workspace known_failures -- --list 2>/dev/null | rg '^known_failures::'

[windows]
known-failures:
  @cargo test --workspace known_failures -- --list 2>nul | rg '^known_failures::'
# Lint the whole project
lint:
  cargo clippy --locked -- --deny warnings

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

# Manage registry-mock
registry-mock +args:
  cargo run --bin=pacquet-registry-mock -- {{args}}

integrated-benchmark +args:
  cargo run --bin=integrated-benchmark -- {{args}}

cli +args:
  cargo run --bin pacquet -- {{args}}
