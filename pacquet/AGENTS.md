# AGENTS.md (pacquet)

Guidance for AI coding agents working in `pacquet/`.

**Read [`../AGENTS.md`](../AGENTS.md) first.** It covers the conventions that apply across the whole monorepo — GitHub PR workflow, signing agent-authored content, conventional commit messages, code-reuse philosophy, "never ignore test failures," and the PR-conflict resolution script. This file specializes those rules for pacquet's Rust code and adds pacquet-only ones.

## What this project is

`pacquet` is a port of the [pnpm](https://github.com/pnpm/pnpm) CLI from
TypeScript to Rust. It is not a new package manager and not a reimagining —
its behavior, flags, defaults, error codes, file formats, and directory layout
are meant to match pnpm exactly.

## The cardinal rule

**Any change in pacquet must match how the same feature is implemented in
the TypeScript pnpm CLI (the workspaces outside `pacquet/`).** The inverse
obligation — user-visible changes to the TypeScript pnpm CLI must also land
in pacquet — lives in [`../AGENTS.md`](../AGENTS.md#keep-pnpm-and-pacquet-in-sync).

Before writing code for a feature, bug fix, or behavior change:

1. Find the equivalent code in the TypeScript pnpm workspaces. They live
   at the repo root — `pnpm/` (CLI entry), `pkg-manager/`, `resolving/`,
   `lockfile/`, `store/`, `fetching/`, `config/`, `hooks/`, and so on.
   See the repo-structure section in
   [`../AGENTS.md`](../AGENTS.md#repository-structure) for the full list.
2. Read the pnpm implementation — logic, edge cases, config resolution,
   error messages, file/lockfile formats, and existing tests.
3. Port the behavior faithfully. Prefer structural similarity (same function
   decomposition, same names where reasonable) so future cross-referencing
   stays cheap.
4. Do not invent behavior that pnpm does not have. Do not "fix" pnpm quirks
   unless the same fix has landed in pnpm.
5. If pnpm and pacquet disagree, pnpm is the source of truth — reconcile
   toward pnpm, not away from it.
6. **Log emissions are part of "match pnpm".** When porting a function
   that fires `pnpm:<channel>` events through `globalLogger` /
   `logger.debug(...)` / `streamParser.write(...)`, mirror the call
   site, payload, and ordering so `@pnpm/cli.default-reporter` parses
   pacquet's NDJSON the same way it parses pnpm's. See
   [Reporter / log events](./CODE_STYLE_GUIDE.md#reporter--log-events)
   in the style guide for the convention (channel mapping, threading
   `R: Reporter`, emit-site placement, recording-fake tests).

If the pnpm behavior is unclear or looks wrong, stop and ask the user
rather than guessing.

When citing code anywhere — code comments, doc comments, Markdown docs,
PR descriptions, or commit messages — link to a specific commit SHA, not
a branch name. Branch links such as `github.com/<owner>/<repo>/blob/main/...`
or `.../tree/master/...` are *impermanent*: their target drifts as the branch
moves and may eventually 404 if the file is renamed or deleted. Permanent
links pin the commit (`github.com/<owner>/<repo>/blob/<sha>/...`) so the
reference stays meaningful long after the code changes. Use the **first 10
hex characters** of the SHA — full 40-character SHAs make URLs unwieldy on
narrow displays and in commit logs, and 10 characters is more than enough to
disambiguate a commit in any real-world repository. Resolve the SHA with
`git log -1 --format=%h` for an in-repo file, or `git ls-remote
https://github.com/<owner>/<repo>.git refs/heads/<branch>` for an external
repo (then take the first 10 characters), or by clicking "Copy permalink"
(`y`) on GitHub and trimming the SHA segment. The rule applies to every
GitHub repository, including this one.

## Porting branded string types

TypeScript pnpm leans on *branded* string types. A branded string is a
plain string narrowed by a phantom property (for example,
`type PkgName = string & { __brand: 'PkgName' }`), so the type system can
track intent that the runtime cannot see. Some brands are stamped through
a validating constructor. Others are minted with a bare `as` type assertion and
have no runtime check at all. Pacquet must preserve that distinction,
because it is part of the public contract pnpm exposes through manifest,
lockfile, state, and config files.

Rules when porting code that uses a branded string type:

1. **Declare a newtype wrapper.** Do not collapse the brand into a plain
   `String` or `&str`. Give the type its own struct so misuse is a type
   error in pacquet too.
2. **If upstream always validates before construction, validate too.**
   When every brand site in pnpm runs through a checking factory, pacquet's
   wrapper must construct only via `TryFrom<String>` and/or `FromStr`. Do
   not provide an infallible public constructor that takes an arbitrary
   string.
3. **If upstream never validates, just brand for type-safety.** Some
   upstream brands exist purely to keep the type system from confusing
   one string slot with another. For example, a brand may exist to prevent
   a `PkgId` from being passed where a `PkgName` is expected, even though
   the value is never validated at runtime. In that case the Rust wrapper
   should expose an infallible `From<String>` (and `From<&str>` when
   convenient). The type-safety win is the whole point, and no validator
   is needed.
4. **If upstream occasionally constructs without validation, expose
   `from_str_unchecked`.** When pnpm sometimes mints the brand via a bare
   `as` assertion, skipping its validator, add a `from_str_unchecked` (or
   similarly named) constructor on the Rust side so callers can opt into
   the same unchecked path explicitly. Keep the validating constructor as
   well. `from_str_unchecked` is the escape hatch, not the default.
5. **Match upstream serde behavior.** If the branded type crosses a
   JSON, YAML, or INI boundary (manifest files, lockfiles, state files,
   config files, and similar), wire the wrapper into serde so the
   validation policy survives serialization:
   - `#[serde(try_from = "String")]` for deserialization, so
     deserialized values go through the validator.
   - `#[serde(into = "String")]` for serialization.
   Use both when the type is round-tripped.
6. **Derive simple conversions with `derive_more`.** When the conversion
   impls implied by the rules above are mechanical (a one-liner that
   wraps or unwraps the inner field), use `#[derive(derive_more::From)]`
   and `#[derive(derive_more::Into)]` rather than handwriting an `impl`
   block. Fall back to a manual `impl` only when the conversion needs
   custom logic, such as validation or normalization. `derive_more` is
   already a workspace dependency.
7. **String-literal unions become `enum`s.** If upstream uses a string
   literal type or a union of string literals (for example,
   `'auto' | 'always' | 'never'`), model it as a Rust `enum`, not a
   newtype wrapper. The set of valid values is closed, so encode that.
8. **Template literal types are branded strings.** If upstream uses a
   string template literal type (for example,
   ``` `${string}@${string}` ```), treat it the same as a branded string
   type. Use a newtype wrapper with the validation discipline from rules
   2 through 5 above.

## Follow the project guides

1. Follow the contributing guide in [`CONTRIBUTING.md`](./CONTRIBUTING.md), and **ALWAYS** double-check before committing. It covers commit message format, writing style, setup, and the automated checks to run before committing.
2. Follow the code style guide in [`CODE_STYLE_GUIDE.md`](./CODE_STYLE_GUIDE.md), and **ALWAYS** double-check before committing. It covers code-level conventions not enforced by tooling: imports, modules, naming, ownership and borrowing, parameter type selection, trait bounds, pattern matching, `pipe-trait`, error handling, test layout, logging during tests, and cloning of `Arc` and `Rc`.

## Repo layout (inside `pacquet/`)

- `crates/` — library and binary crates that make up pacquet.
  - `cli`, `package-manager`, `package-manifest`, `lockfile`, `store-dir`,
    `tarball`, `registry`, `network`, `npmrc`, `fs`, `executor`,
    `diagnostics`, `testing-utils`.
- `tasks/` — developer tooling: `integrated-benchmark`, `micro-benchmark`,
  `registry-mock`.
- `CONTRIBUTING.md` — commit-message format, writing style, setup, and the
  automated checks to run before submitting. Read it before submitting code.
- `CODE_STYLE_GUIDE.md` — manual code-style conventions beyond what `cargo
  fmt`, `taplo`, and clippy enforce: imports, modules, naming, ownership
  and borrowing, trait bounds, pattern matching, `pipe-trait`, error
  handling, test layout, and `Arc`/`Rc` cloning. Read it before submitting
  code.

The Rust workspace (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
`justfile`, `.cargo/`, `.taplo.toml`, etc.) lives at the **repo root**, not
inside `pacquet/`. Run `cargo` and `just` from the repo root.

## Commands

Prefer `just` recipes when one fits; drop down to `cargo` / `taplo` / etc.
directly when you need flags the recipe doesn't expose (e.g. filtering tests
by crate or name — see below).

- `just ready` — run the same checks CI runs (typos, fmt, check, test, lint).
  Run this before declaring a task complete.
- `just test` — `cargo nextest run`.
- `just lint` — `cargo clippy --locked -- --deny warnings`.
- `just check` — `cargo check --locked`.
- `just fmt` — `cargo fmt` + `taplo format`.
- `just cli -- <args>` — run the pacquet binary.
- `just registry-mock <args>` — manage the mock registry used by tests.
- `just integrated-benchmark <args>` — compare revisions or compare against
  pnpm itself (see `CONTRIBUTING.md`).

Warnings are errors (`--deny warnings` in lint). Do not silence them with
`#[allow(...)]` unless there is a specific, justified reason.

## Tests

- Tests live alongside the code they exercise (standard Cargo layout) plus
  integration tests under each crate's `tests/`. Shared test fixtures live
  under `crates/testing-utils/src/fixtures/`.
- Snapshot tests use `insta`. When an intentional change alters a snapshot,
  review the diff carefully, then accept with `cargo insta review`. Never
  accept snapshot changes blindly.
- Some tests require the mocked registry. Start it with
  `just registry-mock launch` if a test needs it.
- When porting behavior from pnpm, port the relevant pnpm tests too (as Rust
  tests) whenever they translate. Matching test coverage is the easiest way
  to prove behavioral parity.
- The active test-porting plan lives in
  [`plans/TEST_PORTING.md`](./plans/TEST_PORTING.md). It enumerates the
  upstream TypeScript tests scheduled to be ported (with file paths and line
  numbers) and the conventions expected of the ports — `known_failures`
  modules, `pacquet_testing_utils::allow_known_failure!` at the
  not-yet-implemented boundary, and the practice of temporarily breaking the
  subject under test to verify the ported test actually catches the
  regression. Consult it before adding ported tests, and update its
  checkboxes as items land.

### Running tests narrowly

Running the full suite is slow. While iterating, target what you're working
on:

```sh
# One crate
cargo nextest run -p pacquet-lockfile

# One test by name substring
cargo nextest run -p pacquet-lockfile <name_substring>

# One integration test file
cargo nextest run -p pacquet-lockfile --test <file_stem>
```

Run `just ready` (full suite) before handing the PR off.

## Style

`CODE_STYLE_GUIDE.md` is the source of truth. Highlights:

- Choose owned vs. borrowed parameters to minimize copies; widen to the most
  encompassing type (`&Path` over `&PathBuf`, `&str` over `&String`) when it
  doesn't force extra copies.
- Prefer `Arc::clone(&x)` / `Rc::clone(&x)` over `x.clone()` for reference-
  counted types, so the cost is visible at the call site.
- Follow the test-logging guidance in the style guide — log before non-
  `assert_eq!` assertions, `dbg!` complex structures, skip logging for simple
  scalar `assert_eq!`.
- Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/naming.html)
  for naming.
- **No star imports inside module bodies.** Write `use super::{Foo, bar}`
  instead of `use super::*;`, and the same for any other glob whose
  target is a module you control. Two forms stay allowed: external-crate
  preludes such as `use rayon::prelude::*;` and root-of-module
  re-exports such as `pub use submodule::*;` in a `lib.rs`. See the
  "No star imports" section in `CODE_STYLE_GUIDE.md`.

### Preserve existing method chains

When editing existing code, do not break a method chain (including `pipe-trait`
`.pipe(...)` chains) into intermediate `let` bindings unless you can justify
the rewrite. Valid justifications include a chain that fails to compile after
your edit, a borrow checker rejection, a meaningful performance win from
splitting it up, or any other concrete reason the chain cannot stay as it is.
Refactoring for style alone is not a justification when the task is something
else. Keep the surrounding code shape intact and confine your edits to what
the task asks for.

When the change you need can fit inside the existing chain, keep it there.
For example, swapping a `PathBuf::from` allocation for a `Path::new` borrow:

```diff
 output
     .stdout
     .pipe(String::from_utf8)
     .expect("convert stdout to UTF-8")
     .trim_end()
-    .pipe(PathBuf::from)
+    .pipe(Path::new)
     .parent()
     .expect("parent of root manifest")
     .to_path_buf()
```

Do not flatten the chain just because you happen to be editing nearby:

```diff
-output
-    .stdout
-    .pipe(String::from_utf8)
-    .expect("convert stdout to UTF-8")
-    .trim_end()
-    .pipe(PathBuf::from)
-    .parent()
-    .expect("parent of root manifest")
-    .to_path_buf()
+let stdout = String::from_utf8(output.stdout).expect("convert stdout to UTF-8");
+Path::new(stdout.trim_end()).parent().expect("parent of root manifest").to_path_buf()
```

If you do need to break a chain, state the justification in your reply, the
commit message, or the PR description so a reviewer can confirm the rewrite
was warranted. If the rewrite is purely stylistic, raise it with the user as
its own change rather than including it in an unrelated edit.

## Code reuse (pacquet specifics)

The general "search before you write / extract shared code / prefer mature
crates / keep deps at the right level" rules from
[`../AGENTS.md`](../AGENTS.md#code-reuse-and-avoiding-duplication) apply.
Pacquet-specific notes:

- Shared helpers tend to live in `crates/fs`, `crates/testing-utils`, and
  `crates/diagnostics` — check there first.
- Check whether the workspace already depends on something suitable (see
  `[workspace.dependencies]` in the root `Cargo.toml`) before adding a new
  dependency.
- **Keep dependencies at the right level.** Add a new dependency to the
  specific crate that needs it, not to the workspace root or to a shared
  crate unless multiple crates actually depend on it.

## Errors and diagnostics

User-facing errors go through `miette` via the `pacquet-diagnostics` crate.
Match pnpm's error codes and messages where pnpm defines them — error codes
are part of the public contract, not implementation detail. See
<https://pnpm.io/errors> for the canonical list.

## Commit and PR hygiene

- Keep commits focused. A bug fix commit should not also refactor or
  reformat unrelated code.
- Reference the upstream pnpm commit/PR you ported from, when applicable.
- Run `just ready` before pushing.
- The repo installs a pre-push hook via `just install-hooks` that runs
  `rustfmt` and `taplo`. Make sure your environment can run cargo (the
  hook needs it) before pushing.

### Commit messages

Conventional Commits applies (see
[`../AGENTS.md`](../AGENTS.md#commit-messages) for the full type list). Use
a scope that names the crate or area being touched, matching the existing
history (`git log --oneline` for examples). Pacquet adds one type beyond the
standard list:

- `bench`: benchmark-only changes.

Examples (from this repo's history):

```
fix(network): set explicit timeouts on default reqwest client
feat(lockfile): support npm-alias dependencies in snapshots
perf(store-dir): share one read-only StoreIndex across cache lookups
```

## Things not to do

- Do not add features, flags, or behaviors that pnpm does not have.
- Do not change lockfile format, store layout, `.npmrc` semantics, or CLI
  surface unless pnpm changed them first.
- A dependency that is already declared in `[workspace.dependencies]` in the
  root `Cargo.toml` may be added to any crate that needs it.
- Do not add a dependency that is not already declared in the workspace
  without an explicit human request. If there is a clear benefit and
  justification for pulling in a new third-party crate, ask the human to
  approve it and to add it to `[workspace.dependencies]` rather than adding
  it yourself. Consult `deny.toml` when evaluating candidates.
- Do not introduce `unsafe` without a clear justification and review.
- Do not disable lints, tests, or CI checks to make a PR green.
