---
name: testing-changes
description: Pick and run the smallest set of pnpm tests that covers a change, in the Rust workspace (pnpm/, pnpr/) or the TypeScript CLI (pnpm11/), including how to derive the affected crates from the diff, and when the full suite is actually required. Use whenever verifying a change before committing or pushing, or when deciding what to run after an edit.
---

# Testing a change

The repository has 11,515 Rust test functions and 700 TypeScript test files. Running all of them for a one-crate change costs an hour of machine time and tells you nothing the scoped run didn't. CI runs the full Rust suite on Linux, macOS, and Windows for every pull request, and the full TypeScript suite too. Locally, run what covers the change.

| Tier | When | Rust | TypeScript |
| --- | --- | --- | --- |
| Iterating | After each edit | The crate you edited, or a single test by name | One test file, or one case with `-t` |
| Before commit or push | Always | `typos pnpm pnpr`, `just fmt`, `just check`, `just lint`, plus the tests affected by the diff | `pnpm --filter <pkg> run compile`, `pnpm run lint`, the touched packages' tests |
| Everything | Only for a change that reaches past the crates you can name | `just ready` | — |

Scope the tests, not the rest. `just check` and `just lint` stay workspace-wide: both cost far less than the test run and they catch cross-crate breakage a `-p` selection hides.

## Rust: which tests does this diff affect?

Run `cargo` and `just` from the repository root; the Rust workspace lives there, not inside `pnpm/`.

Cargo has no test-impact analysis, and nextest has no `changed()` predicate. What it has is `package()` and `rdeps()`, and in this workspace only the first one scopes anything.

`just test-affected` maps the working tree's changes to crates and runs those crates' tests:

```sh
just test-affected                     # against main
just test-affected --base HEAD~3       # against another revision
just test-affected --print             # show the selection, run nothing
```

It selects every `pnpr-*` crate together when any of them changed, and it refuses to guess a subset when `Cargo.lock`, the workspace manifest, the toolchain, the nextest config, or `pnpm-testing-utils` changed — those affect every crate, so it points at `just ready` instead.

For each selected crate it also reports how many crates depend on it whose tests are not in the selection, so you can see what you are leaving to CI:

```
Testing 1 crate(s) changed against main:
  pnpm-fs (68 crates depend on it; their tests are not selected)
```

What it cannot decide for you is the CLI end-to-end suite. `pnpm-cli` is only in the selection when you changed it, so for a user-visible change add the suite modules that exercise the area:

```sh
just test-affected -- -p pnpm-cli -E 'test(catalog::)'
```

Each file under `crates/cli/tests/suite/` is a module of one test target, so `test(<file_stem>::)` selects that file's tests. Read the file names and take the ones that exercise the changed behavior.

Use `-p <crate>` rather than a `package()` filterset when you are choosing crates by hand. `-p` restricts what cargo builds; a filterset only selects among the binaries that were built anyway.

### Why not `rdeps()`

`rdeps(<crate>)` selects the crate and everything that transitively depends on it, which sounds like the right answer and is not. `pnpm-cli` holds 3,712 of the workspace's 11,515 test functions and depends on nearly every crate, and `pnpm-testing-utils` is a dev-dependency of most crates while depending on `pnpr`, which joins the two products into one graph. The result, measured:

| Changed crate | Tests `rdeps()` selects |
| --- | --- |
| `pnpm-fs` | 93% of the suite |
| `pnpm-lockfile` | 88% |
| `pnpm-config` | 84% |
| `pnpr-storage` | 84% |

For a crate anywhere near the core, `rdeps()` is the full suite wearing a filterset. Use it only for a crate at the edge of the graph, and check what it selected with `cargo nextest list -E '<expr>'` before committing to the run.

Changing this — splitting the CLI end-to-end target so selection means something — is tracked in [pnpm/pnpm#14984](https://github.com/pnpm/pnpm/issues/14984).

### What stands in for the dependents

Selecting a crate runs its own tests and nothing downstream, so a change to `pnpm-lockfile` leaves the 59 crates that depend on it unrun, `pnpm-cli` among them. Running those in full is the whole suite; running nothing means a broken command surfaces in CI.

`just test-affected` runs the smoke profile in their place, and says so:

```
Testing 1 crate(s) changed against main:
  pnpm-lockfile (59 crates depend on it)

The crates that depend on those are not selected in full. Running pnpm-cli
smoke tests in their place: one end-to-end test per area of CLI behavior.
```

It happens only when something unselected depends on what changed. Change `pnpm-cli` itself and its full suite runs instead; change a crate nothing depends on and no smoke tests run at all. `--no-smoke` skips them.

`just smoke` runs the same set on its own.

Membership lives in the `smoke` profile in `.config/nextest.toml`, one entry per area of CLI behavior, and `pnpm/scripts/smoke-profile.test.mjs` fails if an entry stops naming a real test. Entries are chosen by behavior area, not by code coverage: nearly every end-to-end test walks the same install path, so a set picked to maximize covered lines would be a few install tests that miss every distinguishing case.

Every entry is a `pnpm-cli` test today, so `pnpm-cli` is the only crate that can stand in for itself this way. A smoke run is a breadth check, not a correctness check: CI runs the whole suite on three platforms before a pull request merges.

### Other selections

```sh
just test-affected                                             # crates the diff touches
just smoke                                                     # one e2e test per area
node pnpm/scripts/run-rust-tests.mjs -p pnpm-lockfile          # one crate
node pnpm/scripts/run-rust-tests.mjs -E 'test(resolves_peer)'  # one test by name
node pnpm/scripts/run-rust-tests.mjs -p pnpm-cli -E 'test(catalog::)'  # one e2e module
just test-pacquet                                              # pacquet crates
just test-pnpr                                                 # pnpr crates
```

Prefer `node pnpm/scripts/run-rust-tests.mjs …` over a bare `cargo nextest run` for anything that exercises the CLI. It is what `just test` invokes: it strips `npm_config_*` and `pnpm_config_*` from the environment and points `XDG_CONFIG_HOME` and the auth npmrc at a throwaway directory. A bare `cargo nextest run` lets your own npmrc and shell config reach the tests, a common source of failures that reproduce for you and nobody else. All arguments pass through to `cargo nextest run`.

### Gotchas that make a scoped run lie

- **`pnpr-*` crates must be selected together.** Cargo unifies features across the selected packages. Selecting one `pnpr-*` crate alone builds it without the backend features `pnpr` enables by default, and its backend tests skip silently. Use `just test-pnpr`, which selects all of them plus `pnpm-registry-mock`.
- **Tests that need the mocked registry start `pnpr` themselves** through `pnpm-testing-utils`. No separate `just registry-mock launch` step. On Windows a running `pnpr.exe` cannot be overwritten, so `just registry-mock` pre-builds with the exact workspace-wide invocation a later `just test` uses.
- **Snapshots.** `insta` snapshots change only for a reason. Read the diff, then `cargo insta review`. Never accept blindly.
- **Killed runs leak fixtures.** A fail-fast or interrupted run abandons temp trees, each holding a per-test store. `just sweep-test-temp` removes the ones older than an hour.
- **`known_failures` modules** hold ported tests for unimplemented behavior; `just known-failures` lists them. A failure there is expected, a pass is not.

### When to run everything

`just ready` (typos, formatter, `just check`, the full `just test`, `just lint`) is for changes where the affected set is not something you can name:

- The workspace `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, or a shared feature flag.
- `pnpm-testing-utils`, `pnpm-diagnostics`, or another crate most of the workspace depends on.
- A rename or signature change that crosses crate boundaries.
- `.config/nextest.toml` or the test harness scripts.

Otherwise let CI run it. It runs the same commands on three platforms.

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

Skip that and the e2e run tests the previous bundle, passing without ever touching your change.

## Both stacks

A bug present in both pnpm v11 and v12 is fixed in both, so it is tested in both. Run the TypeScript test for the scenario and the Rust test for the same scenario before calling the fix done.

## Reporting

Name the selection you ran. "Ran `pnpm-lockfile` and the `catalog` e2e module; did not run the full workspace suite" is an honest report. "Tests pass" after a single-crate run is not.
