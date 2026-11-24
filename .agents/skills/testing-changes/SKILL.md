---
name: testing-changes
description: Run the tests that cover a change in the pnpm repository, in the Rust workspace (pnpm/, pnpr/) or the TypeScript and JavaScript packages and scripts, and recognize the cases where a scoped run passes without testing anything. Use whenever verifying a change before committing or pushing, or when deciding what to run after an edit.
---

# Testing a change

Run what the change affects. CI runs the full suite on three platforms for every pull request, so the local job is fast feedback, not a second gate. The per-commit checks are in [`pnpm/CONTRIBUTING.md`](../../../pnpm/CONTRIBUTING.md#automated-checks).

## Rust (`pnpm/`, `pnpr/`)

Run the tests through the root `package.json` scripts from the repository root: `pnpm test:rust-affected`, `pnpm test:rust`, `pnpm test:rust-smoke`. They wrap the `just` recipes and node scripts named below, pass their arguments through, and let a task concurrency group (`concurrencyGroups` in `pnpm-workspace.yaml`) hold the runs of every worktree on the machine to a limit it can carry. A bare `cargo` or `just` slips past that limit.

Run `pnpm test:rust-affected` (`just test-affected`). It decides three things for you: it selects every `pnpr-*` crate together, it refuses to scope a change that reaches files every crate compiles against and points at `just ready` instead, and it runs the smoke profile in place of dependents it did not select. It prints what it selected and what it left out; `--help` lists its flags.

What it cannot decide is which end-to-end tests exercise *your* change. Smoke gives breadth across areas, not depth in the one you touched, so for a user-visible change add the suite modules for that area:

```sh
pnpm test:rust-affected -- -p pnpm-cli -E 'test(catalog::)'
```

Each file under `crates/cli/tests/suite/` is a module of one test target, so `test(<file_stem>::)` selects that file's tests.

For anything narrower, `pnpm test:rust` (`node pnpm/scripts/run-rust-tests.mjs`) takes the same arguments `cargo nextest run` does — `-p <crate>` for one crate, `-E 'test(<name>)'` for one test. Prefer `-p` over a `package()` filterset: `-p` restricts what cargo builds, a filterset only selects among binaries that were built anyway.

### Gotchas that make a scoped run lie

- **Run the CLI's tests through `pnpm test:rust`, not bare `cargo nextest`.** It strips `npm_config_*` and `pnpm_config_*` and points `XDG_CONFIG_HOME` and the auth npmrc at a throwaway directory, as `just test` does. A bare run lets your own npmrc reach the tests, which fails for you and nobody else.
- **`pnpr-*` crates must be selected together.** Cargo unifies features across the selection, so a lone `pnpr-*` crate builds without `pnpr`'s default backend features and its backend tests skip silently. `just test-pnpr` selects the whole set.
- **Snapshots.** `insta` snapshots change only for a reason. Read the diff, then `cargo insta review`. Never accept blindly.
- **Killed runs leak fixtures.** An interrupted run abandons temp trees, each holding a per-test store. `just sweep-test-temp` clears the ones older than an hour.
- **`known_failures` modules** hold ported tests for unimplemented behavior; `just known-failures` lists them. A failure there is expected, a pass is not.

A change to a core crate no longer has to choose between that crate's own tests and the whole CLI suite. `pnpm test:rust-affected` also runs the CLI end-to-end modules listed for the changed crate in `E2E_AREAS` in `pnpm/scripts/test-affected.mjs`. For `pnpm-lockfile` that is the `add`, `install`, `update` and related modules. The mapping is explicit because the tests spawn the CLI, so no call edge connects a test to the crate it covers. Add a module to the map when a crate's observable effects are asserted in a suite file that is missing from it.

Registry-backed helpers live in `pnpm-testing-utils-registry`, not in the shared `pnpm-testing-utils` harness. Keeping `pnpr` out of the shared crate cut the static `rdeps()` fan-out for core crates. `pnpm-lockfile` went from 88 packages and 95.0% of counted test attributes to 43 packages and 77.1%. `pnpm-config` went from 95.0% to 68.8%, and `pnpr-storage` from 95.0% to 55.2%. Both harness crates are workspace-wide changes, so edits to either refuse scoping.

`pnpm-cli` still sits downstream of the core crates, so `rdeps()` remains wider than the targeted default. Do not use it to widen a day-to-day selection. Use the crate-to-module map and the smoke profile instead. The tradeoff between splitting the suite, sharing registry fixtures, and this mapping is tracked in [pnpm/tasks#18](https://github.com/pnpm/tasks/issues/18).

## TypeScript and JavaScript

```sh
pnpm --filter get-pnpm test                  # builds with tsc, then runs node:test
node --test .github/scripts/bump.test.mjs   # a script's sibling test
```

## Reporting

Name what you ran. "Ran `pnpm-lockfile` plus the `catalog` e2e module; did not run the full workspace suite" is an honest report. "Tests pass" after a single-crate run is not.
