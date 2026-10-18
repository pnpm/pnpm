#!/usr/bin/env bash
# Catch formatter, rustdoc, dylint, and spelling violations before they hit CI.
# Invoked from .husky/pre-push.
set -euo pipefail

red()    { printf '\033[0;31m%s\033[0m\n' "$*" >&2; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$*" >&2; }

failed=0
dylint_pid=

wait_for_dylint() {
    [ -n "$dylint_pid" ] || return 0
    local status=0
    wait "$dylint_pid" || status=$?
    cat "$dylint_log" >&2
    rm -f "$dylint_log"
    if [ "$status" -ne 0 ]; then
        red '✗ cargo dylint reported lints — `just dylint-fix` applies the ones it can, then commit.'
        failed=1
    fi
}

if command -v cargo >/dev/null 2>&1; then
    yellow '▸ node pnpm/scripts/rustfmt.mjs --all -- --check'
    if ! node pnpm/scripts/rustfmt.mjs --all -- --check; then
        red '✗ Rust formatting check failed — run `just fmt` and commit.'
        failed=1
    fi

    if command -v cargo-dylint >/dev/null 2>&1 && command -v rustup >/dev/null 2>&1; then
        yellow '▸ RUSTFLAGS="-D warnings" cargo dylint --all -- --all-targets --workspace'
        # Git exports its repository-local variables (GIT_DIR, GIT_INDEX_FILE,
        # ...) to hooks. When dylint builds a driver for a new toolchain it
        # runs `git checkout` in its own clone of rust-clippy, and an inherited
        # GIT_DIR makes that checkout read this repository instead and fail
        # (https://github.com/trailofbits/dylint/issues/2105).
        #
        # dylint builds into its own target directory, so it runs alongside
        # clippy and doc instead of after them. Its output is held back until
        # they finish so the logs do not interleave.
        dylint_log=$(mktemp)
        # shellcheck disable=SC2046
        (unset $(git rev-parse --local-env-vars) && RUSTFLAGS='-D warnings' node pnpm/scripts/cargo-dylint.mjs --all -- --all-targets --workspace) >"$dylint_log" 2>&1 &
        dylint_pid=$!
    else
        yellow '! cargo-dylint or rustup not found on PATH — skipping dylint check (install rustup, then cargo-dylint from source with `cargo install cargo-dylint dylint-link`).'
    fi

    # Mirror the CI clippy gate. `--all-targets` is the load-bearing flag:
    # without it clippy skips the test and bench crates, so a lint that
    # only fires in an integration test slips past `cargo clippy -p <crate>`
    # and surfaces for the first time in CI.
    yellow '▸ cargo clippy --all-targets --workspace -- -D warnings'
    if ! cargo clippy --all-targets --workspace -- -D warnings; then
        red '✗ cargo clippy reported lints — `just fix` applies the ones it can, then commit.'
        failed=1
    fi

    # `--document-private-items` is what the "Rust CI / Doc" job passes.
    # Nearly every item in these crates is private or `pub(super)`, and
    # rustdoc only resolves the doc links of the items it documents — so
    # without the flag a link to a renamed or deleted private item passes
    # here and fails there.
    #
    # Cargo cannot spawn the shell wrapper on Windows, so the doc step falls
    # back to the stock rustdoc there.
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) rustdoc_bin=rustdoc ;;
        *) rustdoc_bin="$PWD/pnpm/scripts/rustdoc-no-merge.sh" ;;
    esac
    yellow '▸ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features --document-private-items'
    if ! RUSTDOC="$rustdoc_bin" RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --workspace --all-features --document-private-items --quiet; then
        red '✗ cargo doc reported warnings — fix the rustdoc diagnostics and commit.'
        failed=1
    fi

    wait_for_dylint
else
    yellow '! cargo not found on PATH — skipping Rust format, doc, and dylint checks.'
fi

if command -v typos >/dev/null 2>&1; then
    # Same target dirs as the "Rust CI / Spell Check" job in pacquet-ci.yml.
    yellow '▸ typos pnpm pnpr'
    if ! typos pnpm pnpr; then
        red '✗ typos found spelling errors — fix them (accepted words live in .typos.toml) and commit.'
        failed=1
    fi
else
    yellow '! typos not found on PATH — skipping spell check (install with `cargo binstall typos-cli` or via `just init`).'
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
