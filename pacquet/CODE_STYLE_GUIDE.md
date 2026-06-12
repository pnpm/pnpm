# Code Style Guide

## Introduction

Clippy cannot yet detect all suboptimal code. This guide supplements that.

This guide is incomplete. More may be added as more pull requests are reviewed.

This is a guide, not a rule. Contributors may break them if they have a good reason to do so.

## Terminology

[owned]: #owned-type
[borrowed]: #borrowed-type
[copying]: #copying

### Owned type

Doesn't have a lifetime, neither implicit nor explicit.

*Examples:* `String`, `OsString`, `PathBuf`, `Vec<T>`, etc.

### Borrowed type

Has a lifetime, either implicit or explicit.

*Examples:* `&str`, `&OsStr`, `&Path`, `&[T]`, etc.

### Copying

The act of cloning or creating an owned data from another owned/borrowed data.

*Examples:*
* `owned_data.clone()`
* `borrowed_data.to_owned()`
* `OwnedType::from(borrowed_data)`
* `path.to_path_buf()`
* `str.to_string()`
* etc.

## Guides

### Naming convention

Follow [the Rust API guidelines](https://rust-lang.github.io/api-guidelines/naming.html). Specific naming conventions for generics, variables, and closure parameters are covered in the sections below.

### Module Organization

- Use the flat file pattern (`module.rs`) rather than `module/mod.rs` for submodules. Enforced by [`clippy::mod_module_files`](https://rust-lang.github.io/rust-clippy/master/index.html#mod_module_files), which bans `mod.rs` files. (`perfectionist::flat_module_pattern` covered this previously and is being retired in favor of the Clippy rule.)
- List `pub mod` declarations first, then `pub use` re-exports, then private imports and items.
- Use `pub use` to re-export key types at the module level for convenience.

```rust
pub mod install_package_by_snapshot;
pub mod install_package_from_registry;
pub mod install_without_lockfile;

pub use install_package_by_snapshot::InstallPackageBySnapshot;
pub use install_package_from_registry::InstallPackageFromRegistry;
```

### Import Organization

Prefer **merged imports**. Combine multiple items from the same crate root into a single `use` statement with nested braces rather than separate `use` lines (the `crate` granularity). Import ordering is enforced by `cargo fmt`; the granularity is enforced by [`perfectionist::import_granularity`](https://github.com/KSXGitHub/perfectionist/blob/0.0.0-rc.17/rules/import_granularity.md) (configured to `crate` in `dylint.toml`). Imports gated by a platform attribute such as `#[cfg(unix)]` go in a separate block after the main imports.

```rust
use crate::{
    package_manager::PackageManager,
    store::Store,
};
use pipe_trait::Pipe;
use std::{path::PathBuf, sync::Arc};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
```

### No star imports

Avoid star (glob) imports inside the bodies of regular modules. Import items explicitly by name everywhere except the two cases noted below. The rule applies to production code, tests, integration tests, build scripts, and developer tooling under `tasks/`.

The two exceptions are:

1. **External-crate preludes**, such as `use rayon::prelude::*;` or `use assert_cmd::prelude::*;`. The upstream crate has already curated which items are intended to be glob-imported, so listing them out by hand creates a maintenance burden the moment the upstream prelude changes. Use the prelude in the form the crate documents.
2. **Re-exports at the root of a module or crate**, such as `pub use submodule::*;` in `lib.rs`. These are part of the public surface that the crate intentionally exposes, and listing the items individually duplicates information that already lives in the submodule.

Star imports inside a module body are the case worth banning. They make it hard to tell where a name comes from, they hide accidental shadowing, and `use super::*;` is especially harmful in tests. The form pulls every privately imported item from the outer module into scope, so an import the production code no longer uses can keep compiling indefinitely as long as some test still references it. Removing dead imports becomes guesswork.

```rust
// Bad
#[cfg(test)]
mod tests {
    use super::*;
}

// Good
#[cfg(test)]
mod tests {
    use super::{ParsedThing, parse_thing};
}
```

```rust
// Allowed (external-crate prelude)
use rayon::prelude::*;
use assert_cmd::prelude::*;
```

```rust
// Allowed (root-of-module re-export)
pub use comver::*;
pub use load_lockfile::*;
```

### Generic Parameter Naming

Use descriptive names for type parameters (`Size`, `Name`, `Manifest`, `Store`, `Reporter`) and `const` generics instead of single letters. Enforced by [`perfectionist::single_letter_generic`](https://github.com/KSXGitHub/perfectionist/blob/0.0.0-rc.17/rules/single_letter_generic.md) and [`perfectionist::single_letter_const_generic`](https://github.com/KSXGitHub/perfectionist/blob/0.0.0-rc.17/rules/single_letter_const_generic.md). The idiomatic `T` for a lone type parameter and `N` for a const-generic array length are accepted at the site with `#[expect(perfectionist::single_letter_generic, …)]` / `#[expect(perfectionist::single_letter_const_generic, …)]`.

### Variable and Closure Parameter Naming

Use descriptive names for variables and closure parameters, in test code as well as production code. Single letters are accepted only where the rules' default allowlists permit them: `n`/`f`/`i`/`j`/`k` for their conventional roles, the `sort_by` / `sort_by_key` / `min_by` / `max_by` / `fold` callback shape, and single-expression closure bodies. Multi-line closure bodies and every `let` binding (including in `#[cfg(test)]` code) are flagged. The same applies to single-letter `const`/`static` items. Enforced by:

- [`perfectionist::single_letter_function_param`](https://github.com/KSXGitHub/perfectionist/blob/0.0.0-rc.17/rules/single_letter_function_param.md)
- [`perfectionist::single_letter_closure_param`](https://github.com/KSXGitHub/perfectionist/blob/0.0.0-rc.17/rules/single_letter_closure_param.md)
- [`perfectionist::single_letter_let_binding`](https://github.com/KSXGitHub/perfectionist/blob/0.0.0-rc.17/rules/single_letter_let_binding.md)
- [`perfectionist::single_letter_const_item`](https://github.com/KSXGitHub/perfectionist/blob/0.0.0-rc.17/rules/single_letter_const_item.md)
- [`perfectionist::single_letter_static_item`](https://github.com/KSXGitHub/perfectionist/blob/0.0.0-rc.17/rules/single_letter_static_item.md)

### When to use [owned] parameter? When to use [borrowed] parameter?

This is a trade-off between API flexibility and performance.

If using an [owned] signature would reduce [copying], one should use an [owned] signature.

Otherwise, use a [borrowed] signature to widen the API surface.

**Example 1:** Preferring [owned] signature.

```rust
fn push_path(list: &mut Vec<PathBuf>, item: &Path) {
    list.push(item.to_path_buf());
}

push_path(&mut my_list, &my_path_buf);
push_path(&mut my_list, my_path_ref);
```

The above code is suboptimal because it forces the [copying] of `my_path_buf` even though the type of `my_path_buf` is already `PathBuf`.

Changing the signature of `item` to `PathBuf` would help remove `.to_path_buf()` inside the `push_path` function, eliminate the cloning of `my_path_buf` (the ownership of `my_path_buf` is transferred to `push_path`).

```rust
fn push_path(list: &mut Vec<PathBuf>, item: PathBuf) {
    list.push(item);
}

push_path(&mut my_list, my_path_buf);
push_path(&mut my_list, my_path_ref.to_path_buf());
```

It does force `my_path_ref` to be explicitly copied, but since `item` is not copied, the total number of copying remains the same for `my_path_ref`.

**Example 2:** Preferring [borrowed] signature.

```rust
fn show_path(path: PathBuf) {
    println!("The path is {path:?}");
}

show_path(my_path_buf);
show_path(my_path_ref.to_path_buf());
```

The above code is suboptimal because it forces the [copying] of `my_path_ref` even though a `&Path` is already compatible with the code inside the function.

Changing the signature of `path` to `&Path` would help remove `.to_path_buf()`, eliminating the unnecessary copying:

```rust
fn show_path(path: &Path) {
    println!("The path is {path:?}");
}

show_path(my_path_buf);
show_path(my_path_ref);
```

### Use the most encompassing type for function parameters

The goal is to allow the function to accept more types of parameters, reducing type conversion.

**Example 1:**

```rust
fn node_bin_dir(workspace: &PathBuf) -> PathBuf {
    workspace.join("node_modules").join(".bin")
}

let a = node_bin_dir(&my_path_buf);
let b = node_bin_dir(&my_path_ref.to_path_buf());
```

The above code is suboptimal because it forces the [copying] of `my_path_ref` only to be used as a reference.

Changing the signature of `workspace` to `&Path` would help remove `.to_path_buf()`, eliminating the unnecessary copying:

```rust
fn node_bin_dir(workspace: &Path) -> PathBuf {
    workspace.join("node_modules").join(".bin")
}

let a = node_bin_dir(&my_path_buf);
let b = node_bin_dir(my_path_ref);
```

### Trait Bounds

Prefer `where` clauses over inline bounds when there are multiple constraints:

```rust
impl<Store, Manifest, Reporter> InstallPackage<Store, Manifest, Reporter>
where
    Store: PackageStore + Send + Sync,
    Manifest: AsRef<PackageManifest> + Send,
    Reporter: ProgressReporter + Sync + ?Sized,
{
    /* ... */
}
```

### Pattern Matching

When mapping enum variants to values, prefer the concise wrapping style:

```rust
ExitCode::from(match self {
    InstallError::NetworkFailure(_) => 2,
    InstallError::ManifestParseFailure(_) => 3,
})
```

### Using `pipe-trait`

This codebase uses the [`pipe-trait`](https://docs.rs/pipe-trait) crate. The `Pipe` trait enables method-chaining through unary functions, keeping code in a natural left-to-right reading order. Import it as `use pipe_trait::Pipe;`.

Any callable that takes a single argument works with `.pipe()`. This includes free functions, closures, newtype constructors, enum variant constructors, `Some`, `Ok`, `Err`, and trait methods such as `From::from`. The guidance below applies equally to all of them.

#### When to use pipe

**Chaining through a unary function at the end of an expression chain:**

```rust
// Good: pipe keeps the chain flowing left-to-right
manifest.dependencies().pipe(DependencyMap)
entries.into_iter().collect::<HashMap<_, _>>().pipe(Store)
```

**Avoiding deeply nested function calls:**

```rust
// Nested calls are harder to read
let parsed = serde_json::from_slice::<Manifest>(&bytes)?;

// Prefer piping instead
let parsed = bytes.as_slice().pipe(serde_json::from_slice::<Manifest>)?;
```

**Chaining through multiple unary functions:**

```rust
name.pipe(InstallError::MissingPackage).pipe(Err)
```

**Continuing a method chain through a free function and back to methods:**

```rust
url
    .pipe(normalize_registry_url)
    .map(Cow::Borrowed)
```

**Using `.pipe_as_ref()` to pass a reference mid-chain.** This avoids introducing a temporary variable when a free function takes `&T`:

```rust
// Good: pipe_as_ref calls .as_ref() then passes to the function
"node_modules"
    .pipe(Path::new)
    .join(package_name)
    .pipe_as_ref(is_within_store)
    .then_some(package_name)
```

#### When NOT to use pipe

**Simple standalone function calls.** Pipe adds noise with no readability benefit:

```rust
// Bad: unnecessary pipe
let result = value.pipe(foo);

// Good: just call the function directly
let result = foo(value);
```

This applies to any unary callable, such as `Some`, `Ok`, or constructors, when there is no preceding chain to continue:

```rust
// Bad: pipe adds nothing here
let result = value.pipe(Some);

// Good: direct call is clearer
let result = Some(value);
```

However, piping through any unary function **is** preferred when it continues an existing chain:

```rust
// Good: continues a chain
manifest.summarize().pipe(Some)
```

### Doc comment intra-links

When a doc comment (`/// ` or `//! `) mentions an identifier (type, trait, function, method, module, constant, macro, etc.) that is intra-linkable from the current scope, write the mention as a [rustdoc intra-doc link][intra-doc-links] rather than as bare prose. Intra-doc links give readers one-click navigation and let rustdoc warn when a referenced item is renamed or removed, so the docs stay in sync with the code.

[intra-doc-links]: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html

```rust
// Good
/// Installs the package described by [`PackageManifest`] into [`Store`].
pub fn install(manifest: &PackageManifest, store: &Store) { /* ... */ }

// Bad: identifiers are mentioned as bare prose
/// Installs the package described by `PackageManifest` into `Store`.
pub fn install(manifest: &PackageManifest, store: &Store) { /* ... */ }
```

If the identifier is not directly in scope, use a path link (`` [`crate::store::Store`] ``) or a disambiguated link (`` [`Store`](crate::store::Store) ``) rather than dropping back to bare prose. Reserve plain backticks for things that genuinely cannot be linked, such as identifiers from external code that rustdoc cannot resolve, shell commands, file paths, or literal values.

### Documentation comments

A doc comment (`///` or `//!`) is rendered by `rustdoc` as the documentation of the item it attaches to, and it is visible to every reader who can see that item. The doc comment of a `pub` item therefore reaches every downstream user of the crate, including users who never read the source. Do not reference items more private than the item being documented. Disallowed references include a private function named in the doc comment of a `pub` item, a `pub(crate)` type named in the doc comment of a `pub` item, and a private constant named in the doc comment of a `pub(crate)` item. A reader who only sees the rendered docs cannot follow such a reference, and intra-doc links to inaccessible items become broken links in `cargo doc` output.

If the explanation genuinely depends on that more private item, choose one of two fixes. The first option is to widen the visibility of the referenced item, adding a re-export when one fits the API. The second option is to move the explanation into a regular `//` comment on the implementation, where readers of the source can see it. Reserve `///` and `//!` for things a downstream user of the item needs to know. Use `//` for notes useful only to someone reading the source.

```rust
// Bad: public doc references a private helper
/// Builds the lockfile by walking dependencies.
///
/// Internally calls [`walk_deps_inner`] to handle cycles.
pub fn build_lockfile(/* ... */) { /* ... */ }

fn walk_deps_inner(/* ... */) { /* ... */ }
```

```rust
// Good: public doc describes observable behavior
/// Builds the lockfile by walking dependencies.
///
/// Cycles in the dependency graph are reported as
/// [`LockfileError::CycleDetected`].
pub fn build_lockfile(/* ... */) { /* ... */ }

fn walk_deps_inner(/* ... */) { /* ... */ }
```

### Serde `Cow<'de, str>` vs `String` source types

When wiring a type into `serde` with `#[serde(try_from = "...")]` or `#[serde(from = "...")]`, pick the source according to whether the deserialized value retains the string.

Do not use `&'de str`. Text formats such as JSON, YAML, and TOML accept escape sequences (for example, JSON's `"\u0061"`) that the deserializer must decode into a fresh buffer. Borrowed `&'de str` deserialization rejects every input that requires decoding, so the type fails on values the format itself accepts.

Prefer `Cow<'de, str>` when the conversion discards the string or splits it into pieces, for example when it parses into a number, an enum discriminant, or a struct whose fields are substrings of the input. The deserializer borrows from the input when no decoding is needed and allocates only when escapes force it.

Prefer `String` when the entire input is moved into the resulting value verbatim. Taking `String` lets the conversion store the buffer directly without re-cloning.

```rust
use std::borrow::Cow;

#[derive(serde::Deserialize)]
#[serde(try_from = "Cow<'de, str>")]
struct Port(u16);

impl<'a> TryFrom<Cow<'a, str>> for Port {
    type Error = std::num::ParseIntError;
    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        value.parse().map(Port)
    }
}

#[derive(serde::Deserialize)]
#[serde(try_from = "String")]
struct PackageName(String);

impl TryFrom<String> for PackageName {
    type Error = InvalidPackageName;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate(&value)?;
        Ok(PackageName(value))
    }
}
```

The same trade-off applies to the infallible `#[serde(from = "...")]` form.

### Error Handling

- Use `derive_more` for error types. Only derive the traits that are actually used:
  - `Display`: derive when the type needs to be displayed, such as when it is printed to stderr or used in format strings.
  - `Error`: derive when the type is used as a `std::error::Error`, such as the error type in `Result` or the source of another error. Not all types with `Display` need `Error`.
  - A type that only needs formatting and not error handling should derive `Display` without `Error`.
- Minimize `unwrap()` in non-test code; use proper error propagation. `unwrap()` is acceptable in tests, and is also acceptable for provably infallible operations when accompanied by a comment explaining the invariant. When deliberately ignoring an error, use `.ok()` and document the rationale.

```rust
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum InstallError {
    #[display("NetworkFailure: {_0}")]
    NetworkFailure(reqwest::Error),
}
```

### Conditional Test Skipping: `#[cfg]` vs `#[cfg_attr(..., ignore)]`

When a test cannot run under certain conditions, such as on the wrong platform, prefer `#[cfg_attr(..., ignore)]` over `#[cfg(...)]` to skip it. The test still compiles on every configuration and is only skipped at runtime. This approach catches type errors and regressions that a `#[cfg]` skip would hide.

Use `#[cfg]` on tests **only** when the code cannot compile under the condition. An example is a test that references types, functions, or trait methods gated behind `#[cfg]` that do not exist on other platforms or feature sets.

Prefer including a reason string in the `ignore` attribute to explain why the test is skipped.

```rust
// Good: test compiles everywhere, skipped at runtime on non-unix
#[test]
#[cfg_attr(not(unix), ignore = "only one path separator style is tested")]
fn unix_path_logic() { /* uses hardcoded unix paths but no unix-only types */ }

// Good: test CANNOT compile on non-unix (uses unix-only types)
#[cfg(unix)]
#[test]
fn unix_permissions() { /* uses PermissionsExt which only exists on unix */ }
```

### When or when not to log during tests? What to log? How to log?

The goal is to enable the programmer to quickly inspect the test subject should a test fail.

Logging is almost always necessary when the assertion is not `assert_eq!`. For example: `assert!`, `assert_ne!`, etc.

Logging is sometimes necessary when the assertion is `assert_eq!`.

If the values being compared with `assert_eq!` are simple scalar or single line strings, logging is almost never necessary. It is because `assert_eq!` should already show both values when assertion fails.

If the values being compared with `assert_eq!` are strings that may have many lines, they should be logged with `eprintln!` and `{}` format.

If the values being compared with `assert_eq!` have complex structures (such as a struct or an array), they should be logged with `dbg!`.

**Example 1:** Logging before assertion is necessary

```rust
let message = my_func().unwrap_err().to_string();
eprintln!("MESSAGE:\n{message}\n");
assert!(message.contains("expected segment"));
```

```rust
let output = execute_my_command();
let received = output.stdout.to_string_lossy(); // could have multiple lines
eprintln!("STDOUT:\n{received}\n");
assert_eq!(received, expected);
```

```rust
let hash_map = create_map(my_argument);
dbg!(&hash_map);
assert!(hash_map.contains_key("foo"));
assert!(hash_map.contains_key("bar"));
```

**Example 2:** Logging is unnecessary

```rust
let received = add(2, 3);
assert_eq!(received, 5);
```

If the assertion fails, the value of `received` will appear alongside the error message.

### Unit test file layout

Always place unit tests in a dedicated external `tests` submodule (`#[cfg(test)] mod tests;`) rather than inline in the parent file — for `src/foo.rs` the tests live in `src/foo/tests.rs`. This keeps production and test code in separate files and avoids churning git blame. Enforced by [`perfectionist::unit_test_file_layout`](https://github.com/KSXGitHub/perfectionist/blob/0.0.0-rc.17/rules/unit_test_file_layout.md) (configured to `inline_style = "external_only"`, nested layout), which flags inline test code, the flattened `src/foo_tests.rs` sibling, and the skipped-intermediate form.

### Cloning `Arc` and `Rc`

Prefer `Arc::clone(&x)` / `Rc::clone(&x)` over `x.clone()` for reference-counted types. The qualified form makes the O(1) refcount bump visible at the call site and fails to compile if a refactor changes the binding's type to something whose `Clone` is an arbitrarily expensive deep copy. Enforced by [`clippy::clone_on_ref_ptr`](https://rust-lang.github.io/rust-clippy/master/index.html#clone_on_ref_ptr), wired into `[workspace.lints.clippy]` in the root `Cargo.toml`.

### Reading process state

Library code should rarely call `std::env::var`, `std::env::var_os`, or `std::env::current_dir` directly. Process state belongs to the running process, not to the operation a function performs, so reaching for it couples the logic to whichever shell invoked pacquet and hides the dependency from the caller. Prefer one of:

- **Take the value as a parameter.** A function that needs a path declares `path: &Path`; one that needs an env-var value takes it as a `&str` argument or an `Option<String>`.
- **Thread the read through the DI seam.** Declare `Sys: EnvVar` (or `EnvVarOs`, `GetHomeDir`, `GetCurrentDir`, …) and call `Sys::var(name)` / `Sys::var_os(name)` / `Sys::home_dir()` / `Sys::current_dir()`. See [Dependency injection for tests](#dependency-injection-for-tests) for the convention. This is how `Config::current` resolves the home dir and the `NPM_CONFIG_WORKSPACE_DIR` lookup, and how `crates/workspace`'s `find_workspace_dir_from_env_with` resolves its env-var lookup — both let tests drive the read without touching the process environment.

The narrow case where a direct call is acceptable is computing the default of a `Config` field that has no caller to take the value from — `default_store_dir`, `default_modules_dir`, and `default_virtual_store_dir` are the canonical examples. Even there, route the lookup through the DI seam: write the helper as `default_store_dir<Sys: EnvVar + GetHomeDir + GetCurrentDir>()` and inline the production composition at the `SmartDefault` site (`#[default(_code = "default_store_dir::<Host>()")]`), so the fake-`Sys` test path and the production `Host` path resolve through the same generic. The other accepted boundary reads are program-entry knobs (`RAYON_NUM_THREADS` in `crates/cli`, `TRACE` in `crates/diagnostics`) and the lifecycle-script env snapshot in `crates/executor` and `crates/git-fetcher`, which has to forward the parent environment to spawned children verbatim. If new library code seems to need `env::var` / `env::current_dir` for any other reason, the answer is almost always a `&Path` (or equivalent) parameter, not a process-state read.

### Dependency injection for tests

Side-effecting code — filesystem access, environment variables, network calls, time, process state — has two testing routes. **The default route is a real fixture:** a `tempfile::TempDir` for filesystem work, the mocked registry (`just registry-mock`) for HTTP, an integration test that spawns the actual pacquet binary in a scratch directory for end-to-end flows. Real fixtures keep tests close to what users see and scale with the codebase without per-call-site plumbing; they are the right tool everywhere except the cases enumerated below.

The dependency-injection seam described below is the **narrow second route**. Reach for it only when one of the following applies:

- **Filesystem error branches the host OS won't reproduce portably.** `PermissionDenied`, `ENOSPC`, a directory that disappears mid-walk, a chmod that fails after the file exists — provoking these on real disks is platform-specific, racy, or both. A fake that returns the exact `io::ErrorKind` is the only portable way to drive the branch.
- **Deterministic time.** Asserting that `prunedAt` equals a specific HTTP-date (RFC 7231 IMF-fixdate, what `httpdate::fmt_http_date` emits), or that a throttled emitter fires on the second sample, needs the clock to be a known value. The real `SystemTime::now` makes those assertions flaky.
- **Shared process-global state that tests would otherwise mutate.** When a branch depends on a single per-process slot — environment variables, the current working directory, the umask, signal handlers, the global allocator — the only way to exercise it without DI is to write to that slot, and the write is observed by every other test in the same process. A serialisation lock (the `EnvGuard` pattern that pnpm/pacquet#343 + pnpm/pnpm#11718 retired from `default_store_dir`) restores correctness only by forcing the affected tests to run single-threaded, and leaves an `unsafe { env::set_var(...) }` block at the call site. A capability-trait fake keeps the read deterministic and the mutation contained to the test that needs it, so nextest's in-process parallelism stays useful and `unsafe` stays out of test code. The reverse follows too: a real-fixture happy path should not be promoted to DI just because it crosses a process-global slot — only the *mutation* side of the slot is the problem; reading whatever the shell already has is fine.
- **External-service happy paths that can't be staged in CI.** Upstream pnpm has features whose *normal* flow depends on real external systems — `pnpm login`'s 2FA prompt round-trip, the OIDC token exchange and provenance attestation in `pnpm publish`, and similar — where the happy path itself is what needs faking, not just the error path. When pacquet ports those features, DI is the right tool for their tests too. (As of writing, these are not yet ported; this exception is documented so the convention is in place when they land.)
- **Unreachable-by-design preconditions.** When a function declares a capability bound but a specific test exercises a branch that never reaches that capability, the fake satisfies the bound with `unreachable!` and documents the precondition. See the worked example below.

A function that takes a `<Sys>` generic but is only ever exercised via real fixtures is a smell — either the DI branches are missing coverage, or the generic is over-design. Either add the tests that justify the seam, or drop the generic and let the real fixture cover everything.

The rest of this section is the convention that applies *when DI is the right tool*.

#### Names

- The generic type parameter is named **`Sys`** — short for "system seam," the slot in the function signature that selects between the real OS and the test fake. A single short name makes a generic call site instantly recognisable as the DI seam.
- The production provider struct is named **`Host`** — unqualified, because the production implementation is the default. Fakes carry behaviour-based names that describe what they do (`FailingRead`, `EmptyRead`, `PermissionDenied`, `FakeHostName`), not what category of thing they are.
- Capability traits use the form `<Domain><Action>`: filesystem capabilities are `Fs*` (`FsReadToString`, `FsCreateDirAll`, `FsWrite`, `FsReadDir`, `FsWalkFiles`, `FsSetExecutable`, `FsEnsureExecutableBits`); environment-variable lookup is `EnvVar`; clock reads are `Clock`; hostname lookup is `GetHostName`. The domain prefix lets a reader of a generic bound see which side effect the function reaches for without chasing definitions. Method names mirror their `std` equivalents so the trait is a thin seam over `std::fs::*` / `std::env::var` / `SystemTime::now`, not a re-imagining.

#### Eight principles

1. **Single-purpose traits.** Each capability gets its own trait — one method per side effect, no umbrella trait that bundles `read`, `write`, `create_dir_all` into one bag. A function then binds only the capabilities it actually consumes, and a test fake implements only the methods the function under test exercises.

2. **One generic parameter with multiple bounds.** Compose bounds on a single `Sys`, never introduce a second type parameter per capability:

   ```rust
   // Good: one parameter, composed bounds
   pub fn read_modules_manifest<Sys>(modules_dir: &Path) -> Result<...>
   where
       Sys: FsReadToString + Clock,
   { /* ... */ }

   // Bad: a parameter per capability — every call site has to satisfy two slots
   pub fn read_modules_manifest<Fs, C>(modules_dir: &Path) -> Result<...>
   where Fs: FsReadToString, C: Clock { /* ... */ }
   ```

   One parameter keeps turbofish call sites short (`read_modules_manifest::<Host>(dir)`) and makes the fake's job obvious: implement every trait in the bound list, no more.

3. **Static methods, not `&self`.** Capability methods are associated functions (no `&self` receiver). The provider is a unit struct that carries no data:

   ```rust
   pub trait FsReadToString {
       fn read_to_string(path: &Path) -> io::Result<String>;
   }

   pub struct Host;
   impl FsReadToString for Host {
       fn read_to_string(path: &Path) -> io::Result<String> { fs::read_to_string(path) }
   }
   ```

   Stateful fakes (a fake clock that returns a fixed `SystemTime`, a recording fake that captures every call) store their state in an interior-mutable `static` declared inside the `#[test]` body, so the trait shape doesn't have to change to accommodate state. This keeps the production impl free of `&self` plumbing the test-only fake would otherwise force on it.

4. **Associated types for data operations.** When a capability operates over a domain data type, expose the data type as an associated type rather than threading an instance through every call. The provider chooses the concrete type, and fakes can pick a stub-friendly stand-in.

5. **Capability traits on the implementor.** `impl FsReadToString for Host` lives on the provider, not on the data type. The data types stay free of test-shim conditional impls; the seam is the provider.

6. **Domain-neutral provider, domain-scoped traits.** The generic is `Sys`, the production type is `Host` (or whatever your crate exports as its provider), and the trait names carry the domain prefix (`Fs*`, `Env*`, `Clock`, `GetHostName`, …). A reader of `Sys: FsReadToString + Clock + EnvVar` knows immediately which side effects the function reaches for.

7. **Explicit turbofish in production.** Production call sites name the provider:

   ```rust
   read_modules_manifest::<Host>(modules_dir)
   write_modules_manifest::<Host>(modules_dir, manifest)
   Config::current::<Host, _, _, _, _>(env::current_dir, home::home_dir, Default::default)
   ```

   The turbofish makes the production choice visible at the call site instead of relying on type inference; if a future caller wants to swap in a different provider (a test driver, a dry-run shim), the spot to change is obvious.

8. **Capabilities are primitives, not algorithms.** Each trait names a leaf-level effect that maps to a single `std` function (`read_to_string`, `create_dir_all`, `write`, `var`, …). Higher-level guarantees — atomic write, retry loops, walk-with-options — become free functions composed on top of those primitives, not new trait methods. That keeps the fake surface dead-simple: a fake declares one method per capability, never a knob-laden builder.

#### Worked example: `modules-yaml`

`crates/modules-yaml` reads and writes `node_modules/.modules.yaml`. The read path is generic over `FsReadToString + Clock` because it needs to read the file and stamp `prunedAt` from the wall clock; the write path is generic over `FsCreateDirAll + FsWrite` because it needs to ensure the parent directory exists before writing the serialized manifest:

```rust
pub trait FsReadToString {
    fn read_to_string(path: &Path) -> io::Result<String>;
}

pub trait FsCreateDirAll {
    fn create_dir_all(path: &Path) -> io::Result<()>;
}

pub trait FsWrite {
    fn write(path: &Path, contents: &[u8]) -> io::Result<()>;
}

pub trait Clock {
    fn now() -> SystemTime;
}

pub struct Host;

impl FsReadToString for Host {
    fn read_to_string(path: &Path) -> io::Result<String> { fs::read_to_string(path) }
}
impl FsCreateDirAll for Host {
    fn create_dir_all(path: &Path) -> io::Result<()> { fs::create_dir_all(path) }
}
impl FsWrite for Host {
    fn write(path: &Path, contents: &[u8]) -> io::Result<()> { fs::write(path, contents) }
}
impl Clock for Host {
    fn now() -> SystemTime { SystemTime::now() }
}

pub fn read_modules_manifest<Sys>(modules_dir: &Path) -> Result<Option<Modules>, ReadModulesError>
where
    Sys: FsReadToString + Clock,
{
    let content = match Sys::read_to_string(&manifest_path) { /* ... */ };
    // ...
    manifest.pruned_at = httpdate::fmt_http_date(Sys::now());
    Ok(Some(manifest))
}
```

A test that wants to drive the `PermissionDenied` branch declares a unit-struct fake inside the `#[test]` body, implementing only the capability the function touches:

```rust
#[test]
fn read_propagates_non_not_found_io_error() {
    struct FailingRead;
    impl FsReadToString for FailingRead {
        fn read_to_string(_: &Path) -> io::Result<String> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "mocked"))
        }
    }
    // `read_modules_manifest`'s bound list is `FsReadToString + Clock`,
    // so every fake must satisfy both bounds at the type level — Rust
    // doesn't know that the `prunedAt` branch is unreachable for this
    // input. The convention for capabilities the test won't exercise
    // is a trivial impl whose body is `unreachable!`: the bound is
    // satisfied, and the panic message documents the precondition the
    // test relies on.
    impl Clock for FailingRead {
        fn now() -> SystemTime {
            unreachable!("clock must not be called when read_to_string fails");
        }
    }
    let err = read_modules_manifest::<FailingRead>(Path::new("/")).unwrap_err();
    assert!(matches!(err, ReadModulesError::ReadFile { .. }));
}
```

Stateful fakes (deterministic clock, recording reads) hold their state in a `static` inside the test fn:

```rust
#[test]
fn read_fills_in_pruned_at_when_missing() {
    static FAKE_NOW: SystemTime = SystemTime::UNIX_EPOCH;
    struct FakeClock;
    impl Clock for FakeClock {
        fn now() -> SystemTime { FAKE_NOW }
    }
    // The `static` lives in this fn's scope, so other tests get
    // independent storage and never race on it. The provider type
    // stays an empty unit struct; the state lives next to the test
    // that needs it. Use `SystemTime::UNIX_EPOCH` (a `const`) for a
    // const-initialised static, or `LazyLock` when the desired value
    // needs a runtime constructor.
    // ...
}
```

#### Cross-domain composition

When a function needs capabilities from more than one domain, list them inline on `Sys`:

```rust
fn write_shim<Sys>(target_path: &Path, shim_path: &Path) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString + FsReadHead + FsWrite + FsSetExecutable + FsEnsureExecutableBits,
{ /* ... */ }
```

The provider implements each trait independently, so adding a domain to an existing `Sys` is one more `impl X for Host` block — no churn on the production type beyond the new line, and no churn on existing tests beyond the ones whose fakes now need the new method.

### Reporter / log events

Pacquet's user-facing output mirrors pnpm's: every channel pnpm fires must fire from the corresponding pacquet site, with the same payload shape and the same firing cadence. The reporter lives in `crates/reporter` (the `Reporter` capability trait, the `LogEvent` enum, the `NdjsonReporter` and `SilentReporter` sinks); this section is the convention for porting emissions into ported functions.

#### Finding the upstream emit

In `pnpm/pnpm`, log events come from one of three call shapes:

- `globalLogger.<channel>.<level>(...)` — the ergonomic helpers in `core/core-loggers/src/<channel>Logger.ts`.
- `logger.<level>({ name: 'pnpm:<channel>', ... })` — the raw logger with a name discriminant.
- Direct `streamParser.write(...)` calls — only in pnpm's reporter internals; when porting, you wouldn't write through this.

When porting a function, grep for those patterns *in the upstream files you're porting from* (not workspace-wide). The emit usually sits immediately before or after the side effect the event describes — e.g., `progressLogger.debug({ status: 'fetched' })` after the tarball finishes downloading, before the import step starts.

#### Channel mapping

The channels pacquet currently emits live in `crates/reporter/src/lib.rs`'s `LogEvent` enum. Each variant pins `#[serde(rename = "pnpm:<channel>")]` so the wire string matches upstream byte-for-byte. The enum is *not* an exhaustive list of pnpm's channels — variants are added as pacquet starts emitting them. When porting a function whose upstream emits a channel `LogEvent` doesn't yet have, add the variant first (see "To add a new channel" below). Read the doc comment on each existing variant for the upstream type permalink; channels that fire from a single canonical emit site link that too, while multi-site channels (`Stage` and `Progress`, which span the install lifecycle) only pin the type and let the porter grep the upstream file for the per-status emits.

To add a new channel: extend the enum with a `#[serde(rename = "pnpm:<channel>")]` variant whose payload mirrors the upstream TS shape field-for-field — camelCase via `#[serde(rename_all = "camelCase")]` where applicable, preserving status-tagged-union shapes (see `ProgressMessage` for the pattern). Add a wire-shape unit test to `crates/reporter/src/tests.rs` that asserts the JSON renders exactly what pnpm's TS emitter would.

#### Threading the reporter

A function that emits is generic over `R: Reporter`. Inside, it calls `R::emit(...)` with a `LogEvent` whose variant matches the channel — `R::emit(&LogEvent::Stage(...))`, `R::emit(&LogEvent::Context(...))`, etc.

```rust
fn install_step<R: Reporter>(prefix: String) {
    R::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix,
        stage: Stage::ImportingStarted,
    }));
    // ...
}
```

Production callers turbofish at the entry point:

```rust
Install { /* ... */ }.run::<NdjsonReporter>().await
```

Tests use the no-op `SilentReporter` when they don't care about emits, or a recording fake when they do (see [Testing](#testing-the-emit) below).

The generic monomorphises away — there's no runtime cost. The ergonomic cost is one `<R: Reporter>` per intermediate fn and one turbofish at the production entry point. When threading reaches into a struct, add `<R: Reporter>` to the impl method or carry an install-scoped state field that the relevant emit depends on (see `link_file::log_method_once`'s `&AtomicU8` parameter for an example of the latter — the function dedupes per-install rather than per-process).

#### Where to put the emit

Match the upstream call site's position relative to side effects. pnpm's reporter expects events in a specific order (`resolved` before `fetched`, `importing_started` before any per-package events, etc.). Emitting in the wrong order makes the JS reporter render the "X/Y resolved" counter incorrectly or skip animations entirely.

Cite the upstream permalink (pinned SHA per the cardinal rule in [`AGENTS.md`](./AGENTS.md)) in the code comment next to the emit:

```rust
// `pnpm:context` carries the directories pnpm's reporter prints
// in the install header. Mirrors the upstream emit at
// <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/context/src/index.ts#L196>.
R::emit(&LogEvent::Context(ContextLog {
    level: LogLevel::Debug,
    current_lockfile_exists: false,
    store_dir: config.store_dir.display().to_string(),
    virtual_store_dir: config.virtual_store_dir.to_string_lossy().into_owned(),
}));
```

#### Testing the emit

Use a recording-fake reporter: a unit-struct declared inside the `#[test]` body, recording into a `static Mutex<Vec<LogEvent>>` declared in the same body. Assert the captured sequence. The unit struct keeps the fake's reach narrow (one test fn) and the static mutex makes the recorded events available to the assertion without threading a handle through the fn under test.

```rust
#[tokio::test]
async fn install_emits_pnpm_event_sequence() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    // ... drive the function under test with `::<RecordingReporter>()` ...

    let captured = EVENTS.lock().unwrap();
    assert!(matches!(
        captured.as_slice(),
        [
            LogEvent::Context(_),
            LogEvent::Stage(StageLog { stage: Stage::ImportingStarted, .. }),
            LogEvent::Stage(StageLog { stage: Stage::ImportingDone, .. }),
            LogEvent::Summary(_),
        ]
    ), "unexpected event sequence: {captured:?}");
}
```

The static lives in the test function's own scope, so other tests have independent buffers and never race on it. Reset it at the start of the test anyway, in case the same test is re-run within the same process (nextest does this on retry).

Verify the test catches a regression: temporarily comment out the emit, run the test, observe the sequence assertion fail, then restore the emit. Same discipline `plans/TEST_PORTING.md` calls for.

#### What not to do

- **Don't reformat upstream messages.** Field names and string values are part of the wire contract — change them and `@pnpm/cli.default-reporter` silently drops the record.
- **Don't invent new channels.** If pnpm doesn't have it, pacquet doesn't either. Channels expand only when upstream adds them.
- **Don't emit at higher granularity than pnpm.** Throttling and size gates exist for a reason — see `pacquet-tarball`'s `fetch_and_extract_once`, which gates `pnpm:fetching-progress in_progress` on a known `Content-Length` *and* `>= 5 MB` (`BIG_TARBALL_SIZE`), then throttles to 500ms with leading + trailing edges. That mirrors `lodash.throttle(opts.onProgress, 500)` in upstream's [`remoteTarballFetcher.ts:143-144`](https://github.com/pnpm/pnpm/blob/086c5e91e8/fetching/tarball-fetcher/src/remoteTarballFetcher.ts#L143-L144) exactly.
- **Don't emit at lower granularity, either.** Skipping events the consumer expects (`fetched` after a download succeeds, `imported` after `create_cas_files` Ok) breaks pnpm's reporter counters.

#### Worked example: `pnpm:summary` in `Install::run`

The `Install::run` function brackets the install with `pnpm:stage` events and emits `pnpm:summary` after import completes. Upstream emits `summaryLogger.debug({ prefix })` after each importer's link phase finishes, at [`installing/deps-installer/src/install/index.ts:1663`](https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-installer/src/install/index.ts#L1663).

In pacquet, the same emit lives at the bottom of `Install::run`:

```rust
R::emit(&LogEvent::Stage(StageLog {
    level: LogLevel::Debug,
    prefix: prefix.clone(),
    stage: Stage::ImportingDone,
}));

// `pnpm:summary` closes the install and lets the reporter render
// the accumulated `pnpm:root` events as a "+N -M" block. Must
// come after `importing_done`, matching pnpm's ordering at
// <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-installer/src/install/index.ts#L1663>.
R::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
```

The recording-fake test that pins the sequence lives in `crates/package-manager/src/install/tests.rs::install_emits_pnpm_event_sequence`. The same pattern carries over to every other channel — only the variant name and payload shape change.
