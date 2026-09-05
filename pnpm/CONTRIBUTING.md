# Contributing to pacquet

See also [`CODE_STYLE_GUIDE.md`](./CODE_STYLE_GUIDE.md) for the code style guide.

## Scope and Version Policy

pacquet is pnpm v12 and the target for new feature development. New commands, settings, and other user-visible features are implemented here and are not backported to the TypeScript pnpm v11 CLI under `../pnpm11/`.

For bug fixes, determine which supported versions contain the bug. A bug present in both v11 and v12 must be fixed and tested in both implementations. A bug present in only one version is fixed only in that version. See [`AGENTS.md`](./AGENTS.md) for the full version policy.

Opening an issue first is optional for a clearly scoped change. Open one when the intended user-visible behavior or design is not obvious, or when coordination is needed for work already in progress.

Bug fixes, performance improvements, tests, and documentation may be sent directly as pull requests. New features must follow the repository's normal design, documentation, testing, and changeset requirements.

## Commit Message Convention

This project uses [Conventional Commits](https://www.conventionalcommits.org/).

### Format

```
type(scope): lowercase description
```

### Rules

- **Types:** `feat`, `fix`, `refactor`, `perf`, `docs`, `style`, `chore`, `ci`, `test`, `lint`.
- **Scopes** (optional): a crate name (`cli`, `store`, `tarball`, `registry`, `lockfile`, `npmrc`, `network`, `fs`, `package-manager`, etc.), or another relevant area such as `deps`, `readme`, `benchmark`, or `toolchain`.
- **Description:** always lowercase after the colon, no trailing period, brief (3-7 words preferred).
- **Breaking changes:** append `!` before the colon. For example: `feat(cli)!: remove deprecated flag`.
- **Code identifiers** in descriptions should be wrapped in backticks. For example: `` chore(deps): update `serde` ``.

There are no exceptions to this format. Version release commits follow the same rules as any other commit.

## Writing Style

Write documentation, comments, and other prose for ease of understanding first. Prefer a formal tone when it does not hurt clarity, and use complete sentences. Avoid mid-sentence breaks introduced by em dashes or long parenthetical clauses. Em dashes are a reliable symptom of loose phrasing; when one appears, restructure the surrounding sentence so each clause stands on its own rather than swapping the em dash for another punctuation mark.

## Code Style

See [`CODE_STYLE_GUIDE.md`](./CODE_STYLE_GUIDE.md). Formatting and lint-level rules are enforced by `cargo fmt`, `taplo format`, and `cargo clippy`; the style guide covers everything those tools cannot enforce.

## Dylint / perfectionist

A separate CI job (`Dylint`) runs [perfectionist](https://github.com/KSXGitHub/perfectionist) over the workspace. perfectionist is early, unstable software and is not yet battle-tested, so it can produce false positives and false negatives.

If perfectionist flags code that is actually correct, or fails to flag code its rule description says it should, do not work around the lint silently:

1. Silence the specific finding at the affected site with `#[expect(perfectionist::rule_name, reason = "...")]`. Always include a `reason`, and write it as a sentence explaining why the lint is wrong here. Do not use `#[allow(...)]`; `#[expect]` errors when the suppression is no longer needed, so the workaround disappears once perfectionist is fixed.
2. Open a new issue on [`KSXGitHub/perfectionist`](https://github.com/KSXGitHub/perfectionist/issues/new) describing the false positive or false negative, with a minimal repro, and tag `/cc @KSXGitHub` in the issue body.

The same procedure applies when a perfectionist rule itself is wrong — for example, a rule that flags an idiom the rule's documentation says it should permit. Silence the site with `#[expect(..., reason = "...")]`, link the upstream issue from the `reason` if one already exists, and file the issue if it does not. Do not edit `dylint.toml` to globally disable a rule, and do not pin perfectionist to an older `tag` to dodge a finding.

You can run the same check locally with `just dylint`. It requires `cargo-dylint` and `dylint-link`, which `just init` does not install; install them from source as described under [Rust toolchain and git hooks](../CONTRIBUTING.md#rust-toolchain-and-git-hooks) in the root guide.

## Setup

### Prerequisites

Install these first:

- [`rustup`](https://rustup.rs)
- [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall)
- [`just`](https://just.systems)
- Node.js
- [`pnpm`](https://pnpm.io)
- `git`

The repository root [`CONTRIBUTING.md`](../CONTRIBUTING.md#rust-toolchain-and-git-hooks) covers the Rust toolchain and the tools the git hooks need, including a note on why `cargo-dylint` must be installed from source and why `~/.cargo/bin` has to be on your `PATH`. Read it first, then use the pacquet-specific steps below.

### Install

Install the project's task tools and the git pre-push hook:

```sh
just init
```

`just init` invokes `cargo-binstall` to install `cargo-nextest`, `cargo-watch`, `cargo-insta`, `typos-cli`, `taplo-cli`, `wasm-pack`, and `cargo-llvm-cov`, then installs `cargo-fixit@0.1.15` from source with `cargo install ... --locked` (it has no prebuilt binaries). `cargo-fixit` backs the `just fix` task. The repo-wide `pnpm install` wires up husky, whose `pre-push` hook runs `pnpm/scripts/pre-push-rust.sh` (format, doc, dylint, typos) alongside the TypeScript compile and lint checks.

`just init` does not install the dylint tools. To run the `Dylint` job's checks locally, install `cargo-dylint` and `dylint-link` as described under [Rust toolchain and git hooks](../CONTRIBUTING.md#rust-toolchain-and-git-hooks) in the root guide.

Install the test dependencies:

```sh
just install
```

## Automated Checks

Before submitting, run:

```sh
just ready
```

This runs `typos`, `cargo fmt`, `just check` (which is `cargo check --locked --workspace --all-targets`), `just test` (which is `cargo nextest run`), and `just lint` (which is `cargo clippy --locked --workspace --all-targets -- --deny warnings`), then prints `git status`. CI runs the same commands on Linux, macOS, and Windows.

To let clippy rewrite the lints it can fix automatically, run `just fix` instead of hand-editing each warning:

```sh
just fix
```

`just fix` runs `cargo fixit --clippy --workspace --all-targets --allow-dirty --allow-staged` (via the pinned `cargo-fixit`). It is faster than `cargo clippy --fix` on repeated runs because `cargo fixit` skips the full re-check compile between fix rounds, so iterating on a lint cleanup does not rebuild the workspace each pass. Run `just lint` afterward to confirm no warnings remain (clippy can't autofix everything).

> [!IMPORTANT]
> Run `just ready` before every commit. This rule applies to all changes, including documentation edits, comment changes, and config updates. Any change can break formatting, linting, building, or tests across the supported platforms.

> [!NOTE]
> Integration tests that need the local registry mock start `pnpr` automatically. After dependencies are installed, `cargo test`, `cargo nextest run`, and `just test` should not require a separate registry process.

## Debugging

Set the `TRACE` environment variable to enable trace-level logging for a given module:

```sh
TRACE=pnpm_tarball just cli add fastify
```

## Testing

```sh
just install              # install necessary dependencies
just test                 # run tests
```

When porting tests from the upstream `pnpm/pnpm` TypeScript repository, see
[`plans/TEST_PORTING.md`](./plans/TEST_PORTING.md). It tracks the tests
scheduled for porting (with upstream file paths and line numbers), the
expected layout for not-yet-implemented behavior (`known_failures` modules
guarded by `pnpm_testing_utils::allow_known_failure!`), and the
verification step of temporarily breaking the implementation to confirm a
ported test actually fails for the right reason before committing.

## Benchmarking

First, start a local registry server, such as [verdaccio](https://verdaccio.org/):

```sh
verdaccio
```

Then use the `integrated-benchmark` task to run benchmarks. For example:

```sh
# Compare the branch you are working on against main
just integrated-benchmark --scenario=isolated-linker.fresh-restore.cold-cache.cold-store pacquet@my-branch pacquet@main
```

```sh
# Compare the current commit against the previous commit
just integrated-benchmark --scenario=isolated-linker.fresh-restore.cold-cache.cold-store pacquet@HEAD pacquet@HEAD~
```

```sh
# Compare pacquet of the current commit against pnpm
just integrated-benchmark --scenario=isolated-linker.fresh-restore.cold-cache.cold-store --with-pnpm pacquet@HEAD
```

```sh
# Compare pacquet of the current commit, pacquet of main, and pnpm against each other
just integrated-benchmark --scenario=isolated-linker.fresh-restore.cold-cache.cold-store --with-pnpm pacquet@HEAD pacquet@main
```

```sh
# See more options
just integrated-benchmark --help
```
