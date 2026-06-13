#!/usr/bin/env bash
# Catch formatter, rustdoc, and dylint violations before they hit CI.
# Invoked from .husky/pre-push.
set -euo pipefail

red()    { printf '\033[0;31m%s\033[0m\n' "$*" >&2; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$*" >&2; }

failed=0

if command -v cargo >/dev/null 2>&1; then
    yellow '▸ cargo fmt --all -- --check'
    if ! cargo fmt --all -- --check; then
        red '✗ cargo fmt found unformatted Rust files — run `cargo fmt --all` (or `just fmt`) and commit.'
        failed=1
    fi

    # Mirror the CI clippy gate. `--all-targets` is the load-bearing flag:
    # without it clippy skips the test and bench crates, so a lint that
    # only fires in an integration test slips past `cargo clippy -p <crate>`
    # and surfaces for the first time in CI.
    yellow '▸ cargo clippy --all-targets --workspace -- -D warnings'
    if ! cargo clippy --all-targets --workspace -- -D warnings; then
        red '✗ cargo clippy reported lints — fix the findings (or `just lint`) and commit.'
        failed=1
    fi

    yellow '▸ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features'
    if ! RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --workspace --all-features --quiet; then
        red '✗ cargo doc reported warnings — fix the rustdoc diagnostics and commit.'
        failed=1
    fi

    if command -v cargo-dylint >/dev/null 2>&1; then
        yellow '▸ RUSTFLAGS="-D warnings" cargo dylint --all -- --all-targets --workspace'
        if ! RUSTFLAGS='-D warnings' cargo dylint --all -- --all-targets --workspace; then
            red '✗ cargo dylint reported lints — fix the findings (or `just dylint`) and commit.'
            failed=1
        fi
    else
        yellow '! cargo-dylint not found on PATH — skipping dylint check (install with `cargo binstall cargo-dylint dylint-link`).'
    fi
else
    yellow '! cargo not found on PATH — skipping Rust format, doc, and dylint checks.'
fi

if command -v taplo >/dev/null 2>&1; then
    yellow '▸ taplo format --check'
    if ! taplo format --check; then
        red '✗ taplo found unformatted TOML — run `taplo format` (or `just fmt`) and commit.'
        failed=1
    fi
else
    yellow '! taplo not found on PATH — skipping TOML format check (install with `cargo binstall taplo-cli` or via `just init`).'
fi

if [ "$failed" -ne 0 ]; then
    red ''
    red 'Push aborted. Bypass with `git push --no-verify` if you really need to.'
    exit 1
fi
