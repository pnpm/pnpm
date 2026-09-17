---
name: testing-changes
description: Run the tests that cover a change in the pnpm repository, in the Rust workspace (pnpm/, pnpr/) or the TypeScript CLI (pnpm11/), and recognize the cases where a scoped run passes without testing anything. Use whenever verifying a change before committing or pushing, or when deciding what to run after an edit.
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

Do not reach for nextest's `rdeps()` to widen a selection. `pnpm-cli` holds a third of the workspace's tests and sits downstream of nearly every crate, so `rdeps()` on anything core selects 84% or more of the suite. Restructuring that target is [pnpm/pnpm#14984](https://github.com/pnpm/pnpm/issues/14984).

## TypeScript (`pnpm11/`)

```sh
pnpm --filter <package_name> test                                 # one package
pnpm --filter <package_name> test <file_path>                     # one file
pnpm --filter <package_name> test <file_path> -t <name_pattern>   # one case
```

The end-to-end tests in `pnpm11/pnpm/test/` run the **bundled** `pnpm11/pnpm/dist/pnpm.mjs`, not each package's `lib/`. After changing any TypeScript package, rebuild the bundle before running them:

```sh
pnpm --filter pnpm run compile
```

Skip that and the run tests the previous bundle, passing without ever touching your change.

## Both stacks

A bug present in both pnpm v11 and v12 is fixed in both, so it is tested in both. Run the TypeScript test for the scenario and the Rust test for the same scenario before calling the fix done.

## Reporting

Name what you ran. "Ran `pnpm-lockfile` plus the `catalog` e2e module; did not run the full workspace suite" is an honest report. "Tests pass" after a single-crate run is not.
