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
  cargo binstall cargo-nextest cargo-watch cargo-insta typos-cli taplo-cli wasm-pack cargo-llvm-cov sccache@0.17.0 -y
  # `cargo-fixit` has no prebuilt binaries, so install it from source
  # with `cargo install` (pinned) instead of `cargo binstall`.
  cargo install cargo-fixit@0.1.15 --locked
  node pnpm/scripts/rustfmt.mjs --install

# When ready, run the same CI commands
ready:
  typos pnpm pnpr
  node pnpm/scripts/rustfmt.mjs --all
  just check
  just test
  just lint
  git status

# Update our local branch with the remote branch (this is for you to sync the submodules)
update:
  git pull
  git submodule update --init

# Install necessary dependencies.
# `pnpm/tasks/registry-mock` is a member of the root pnpm workspace,
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
  node pnpm/scripts/rustfmt.mjs --all
  taplo format

# Run cargo check
check:
  cargo check --locked --workspace --all-targets

# Run all the tests.
test:
  node pnpm/scripts/run-rust-tests.mjs

# A test process that is killed cannot run `TempDir`'s cleanup, so a
# fail-fast or interrupted run abandons whole fixture trees — each holding a
# per-test store for the mocked-registry tests, which is what actually adds
# up. Only `pacquet-test-*` is swept: that prefix comes from
# `CommandTempCwd`, so a match is known to be ours. `-mindepth 1` keeps the
# root itself out of the match, and the age floor leaves a concurrent run
# alone.

# Remove fixture trees that earlier test runs abandoned.
sweep-test-temp:
  find "${TMPDIR:-/tmp}" -mindepth 1 -maxdepth 1 -name 'pacquet-test-*' -mmin +60 -exec rm -rf {} + 2>/dev/null || true

# Selection is crate-level: a changed crate's whole test set runs, or none of
# it. `--base <rev>` sets what the diff is taken against (default: the
# remote-tracking `main`, or the local one where no such ref exists),
# `--print` shows the selection without running it, and any other argument is
# passed to nextest: `just test-affected -- -E 'test(catalog::)'`. A change to
# `Cargo.lock`, the workspace manifest, or the toolchain affects every crate,
# so the script refuses to guess a subset and points at `just ready` instead.

# Breadth in seconds, where `just test-affected` gives depth on what you
# changed. Membership is one test per area of CLI behavior; see the comment on
# the `smoke` profile in `.config/nextest.toml`.

# Run one end-to-end test per area of CLI behavior.
smoke *args:
  node pnpm/scripts/run-rust-tests.mjs --profile smoke {{args}}

# Run the tests of the crates the working tree changes.
test-affected *args:
  node pnpm/scripts/test-affected.mjs {{args}}

# With RUST_TEST_SHARED_BUILD=1, test-pacquet and test-pnpr build the whole
# workspace once and pick their tests with a filter. Separate selections make
# cargo unify features differently, so the pnpr step would rebuild most of the
# dependency graph. The shared build compiles pnpr's tests with the features
# pacquet's tests also enable.
pnpr_tests := "package(/^pnpr/) | package(pnpm-registry-mock)"
shared_test_build := env_var_or_default("RUST_TEST_SHARED_BUILD", "")

# Run pacquet package tests only.
test-pacquet:
  node pnpm/scripts/run-rust-tests.mjs {{ if shared_test_build == "1" { "--workspace -E 'not (" + pnpr_tests + ")'" } else { "--workspace --exclude pnpm-registry-mock --exclude pnpr --exclude pnpr-auth --exclude pnpr-cargo --exclude pnpr-config --exclude pnpr-error --exclude pnpr-fixtures --exclude pnpr-oci --exclude pnpr-osv --exclude pnpr-package-name --exclude pnpr-pipeline-runs --exclude pnpr-policy --exclude pnpr-pypi --exclude pnpr-registry --exclude pnpr-route --exclude pnpr-search --exclude pnpr-shared-artifacts --exclude pnpr-storage --exclude pnpr-upstream" } }}

# Run pnpr package tests only.
test-pnpr:
  # Every `pnpr-*` crate, selected together so cargo's feature unification
  # gives them the same backend features `pnpr` itself defaults to — selecting
  # one alone would build it bare and silently skip its backend tests.
  # `pnpm-registry-mock` rides along because its tests spawn a real `pnpr`
  # child, and this is the selection that puts that binary in `target/`.
  cargo nextest run {{ if shared_test_build == "1" { "--workspace -E '" + pnpr_tests + "'" } else { "-p pnpm-registry-mock -p pnpr -p pnpr-auth -p pnpr-cargo -p pnpr-config -p pnpr-error -p pnpr-fixtures -p pnpr-oci -p pnpr-osv -p pnpr-package-name -p pnpr-pipeline-runs -p pnpr-policy -p pnpr-pypi -p pnpr-registry -p pnpr-route -p pnpr-search -p pnpr-shared-artifacts -p pnpr-storage -p pnpr-upstream" } }}

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

# Apply clippy's autofix suggestions across the workspace.
# Uses `cargo fixit --clippy` (installed by `just init`, pinned to
# `cargo-fixit@0.1.15`) instead of `cargo clippy --fix`. `cargo fixit`
# is faster than `cargo clippy --fix` on repeated runs because it skips
# the full re-check compile between fix rounds, so iterating on a lint
# cleanup doesn't rebuild the workspace each pass.
fix:
  cargo fixit --clippy --workspace --all-targets --allow-dirty --allow-staged

# Run perfectionist dylint rules. Requires `cargo-dylint` and `dylint-link`
# (install from source with `cargo install cargo-dylint dylint-link`; the
# prebuilt binstall binaries fail to build the driver locally). The lint
# library is pinned in `dylint.toml`.
dylint:
  env RUSTFLAGS="-D warnings" cargo dylint --all -- --all-targets --workspace

# Apply the dylint suggestions that carry a machine-applicable fix.
dylint-fix:
  cargo dylint --all --fix -- --all-targets --workspace --allow-dirty --allow-staged

# Get code coverage
codecov:
  cargo codecov --html

# Run the benchmarks. See `tasks/benchmark`
micro-benchmark:
  cargo run --bin=micro-benchmark --release

# Compare Rust artifact reuse between two disposable worktrees.
bench-rust-cache *args:
  node pnpm/scripts/bench-rust-cache.mjs {{args}}

# Manage registry-mock. The launcher spawns `pnpr`; on
# Windows you can't overwrite a running .exe, so we pre-build all
# the test artifacts a subsequent `just test` will need with the
# exact same invocation. A `-p pnpr`-scoped pre-build is
# not enough — workspace-wide feature unification gives a
# different fingerprint and nextest would still try to re-link the
# running binary, failing with `os error 5` on Windows MSVC.
registry-mock +args:
  cargo nextest run --no-run
  cargo run --bin=pnpm-registry-mock -- {{args}}

# Compare pnpm's Cargo resolution against cargo's own, on the live index.
cargo-equivalence +args:
  cargo run --bin=cargo-equivalence -- {{args}}

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
  cargo run --bin pnpm -- {{args}}
