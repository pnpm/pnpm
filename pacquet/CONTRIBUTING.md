# Contributing to pacquet

See also [`CODE_STYLE_GUIDE.md`](./CODE_STYLE_GUIDE.md) for the code style guide.

## Scope and Roadmap

pacquet's scope is defined by the roadmap in [#299](https://github.com/pnpm/pacquet/issues/299). The current focus is **Stage 1 — Headless installer**: making `pacquet install --frozen-lockfile` feature-complete with `pnpm install --frozen-lockfile`.

Stage 1 focuses on `pacquet install` and the settings and behavior needed to match `pnpm install --frozen-lockfile`. Other top-level commands exist in the CLI today, but they are not part of Stage 1 and are not receiving feature work, and new top-level commands are out of scope. pacquet is intended to be executed by the pnpm CLI under the hood, so configuration arrives through pnpm settings (such as `.npmrc` and `pnpm-workspace.yaml`) rather than through new command-line flags.

Before opening a pull request that adds a new setting or user-visible feature, **confirm the feature is listed under Stage 1 of the roadmap**. Work that does not appear under Stage 1 will not be reviewed or merged at this time, regardless of implementation quality. Stage 2 and later items are deferred until Stage 1 is complete.

Opening an issue first is optional when the change is in Stage 1 *and* the implementation exactly mirrors how the pnpm CLI works: same behavior, same defaults, same error codes, same file formats. See [`AGENTS.md`](./AGENTS.md) for the parity rule. Open an issue first when the right approach is not obvious from upstream code, or to coordinate on in-flight work.

Deviating from pnpm's behavior is not an option in pacquet. If you believe pnpm itself should change, raise it in the [pnpm repository](https://github.com/pnpm/pnpm) first. Once the change has landed in pnpm and shipped, the corresponding port can be made here.

Bug fixes, performance improvements, tests, and documentation for behavior that already exists do not need a roadmap entry and may be sent directly as pull requests.

Pull requests for new top-level commands, or for features outside the current Stage 1 scope, will be closed with a pointer to the roadmap.

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

You can run the same check locally with `just dylint` (requires `cargo-dylint` and `dylint-link`; install with `cargo binstall cargo-dylint dylint-link`).

## Setup

### Prerequisites

Install these first:

- [`rustup`](https://rustup.rs)
- [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall)
- [`just`](https://just.systems)
- Node.js
- [`pnpm`](https://pnpm.io)
- `git`

### Install

Install the project's task tools and the git pre-push hook:

```sh
just init
```

`just init` invokes `cargo-binstall` to install `cargo-nextest`, `cargo-watch`, `cargo-insta`, `typos-cli`, `taplo-cli`, `wasm-pack`, and `cargo-llvm-cov`, then points `git` at the tracked `.githooks/` directory so the pre-push format check runs on `git push`.

Install the test dependencies:

```sh
just install
```

## Automated Checks

Before submitting, run:

```sh
just ready
```

This runs `typos`, `cargo fmt`, `just check` (which is `cargo check --locked`), `just test` (which is `cargo nextest run`), and `just lint` (which is `cargo clippy --locked -- --deny warnings`), then prints `git status`. CI runs the same commands on Linux, macOS, and Windows.

> [!IMPORTANT]
> Run `just ready` before every commit. This rule applies to all changes, including documentation edits, comment changes, and config updates. Any change can break formatting, linting, building, or tests across the supported platforms.

> [!NOTE]
> Some integration tests require the local registry mock. Start it with `just registry-mock launch` before running `just test` if a test needs it.

## Debugging

Set the `TRACE` environment variable to enable trace-level logging for a given module:

```sh
TRACE=pacquet_tarball just cli add fastify
```

## Testing

```sh
just install              # install necessary dependencies
just registry-mock launch # start a mocked registry server (optional)
just test                 # run tests
```

When porting tests from the upstream `pnpm/pnpm` TypeScript repository, see
[`plans/TEST_PORTING.md`](./plans/TEST_PORTING.md). It tracks the tests
scheduled for porting (with upstream file paths and line numbers), the
expected layout for not-yet-implemented behavior (`known_failures` modules
guarded by `pacquet_testing_utils::allow_known_failure!`), and the
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
just integrated-benchmark --scenario=frozen-lockfile my-branch main
```

```sh
# Compare the current commit against the previous commit
just integrated-benchmark --scenario=frozen-lockfile HEAD HEAD~
```

```sh
# Compare pacquet of the current commit against pnpm
just integrated-benchmark --scenario=frozen-lockfile --with-pnpm HEAD
```

```sh
# Compare pacquet of the current commit, pacquet of main, and pnpm against each other
just integrated-benchmark --scenario=frozen-lockfile --with-pnpm HEAD main
```

```sh
# See more options
just integrated-benchmark --help
```
