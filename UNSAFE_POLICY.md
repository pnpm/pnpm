# `unsafe` policy

This governs every Rust crate in the workspace: pacquet under `pnpm/` and
the registry server under `pnpr/`. It is binding on reviewers and
contributors alike, and [`pnpm/tasks/unsafe-audit`](./pnpm/tasks/unsafe-audit)
enforces the parts of it a test can check.

The rule in one sentence: **a crate either abstracts a foreign or
raw-pointer API, or it keeps `unsafe` out by attribute.** "Just this one
call" is not a third option.

## The tiers

Every crate sits in exactly one tier. Which tier it is in is visible from
its name and from its crate root.

### `-sys` crates: declare the foreign ABI

A `-sys` crate declares a foreign ABI and does nothing else: `unsafe extern`
blocks, the types those signatures name, and the link attributes that bind
them. No error translation, no ownership transfer, no convenience wrapper.

An `unsafe extern` block may be written **only** in a `-sys` crate. The
external `-sys` crates the workspace already depends on count as this tier:
`libc`, `windows-sys`, `libsqlite3-sys` (reached through `rusqlite::ffi`),
and `security-framework-sys`. pnpm's own foreign ABI — the WebContainer
supervisor's imports — lives in
[`pnpm-webcontainer-sys`](./pnpm/crates/webcontainer-sys).

A `-sys` crate is not safe to call and does not pretend to be. It carries
no `#![forbid(unsafe_code)]`, because declaring the ABI is its whole job.

### `pnpm-ffi-*` crates: turn one ABI into a safe API

An FFI abstraction crate exists to make one foreign surface safe to call.
Its `unsafe` blocks are the point of it, not an implementation detail that
leaked in.

It is named `pnpm-ffi-<surface>`, and it must:

- **Expose no way to break it.** No `unsafe fn`, no raw pointer, no raw
  descriptor or handle, no bare `errno` in its public API. A caller who can
  reach undefined behaviour through the safe API has not been abstracted
  from anything.
- **Own what it hands out.** A descriptor becomes a `File` or an `OwnedFd`,
  an allocation becomes an owned Rust value, a `+1` reference becomes a
  guard with a `Drop`. Nothing crosses the boundary that the caller has to
  remember to release.
- **Translate the failure.** A foreign call's status code becomes
  `io::Result`, `Option`, or a crate error before it is returned.
- **Hold nothing but the abstraction.** Product logic belongs in the crate
  that has the product concern. The test for whether a decision belongs
  here is whether it would still be made if the foreign call were already
  safe; if it would, it belongs to the caller.

### Test-support crates: the harnesses tests run under

A fixture that has to open a pseudo-terminal, allocate a console, or drop a
thread's privileges is abstracting a foreign surface for the same reason a
`pnpm-ffi-*` crate is, and under the same rules. It lives in
[`pnpm-testing-utils`](./pnpm/crates/testing-utils) rather than in the test
target that needs it, so that a test target is a consumer like any other.

A test needing a foreign call that production never makes is the one thing
that puts code here. A test needing one that production *does* make means
the abstraction is missing a function, not that the test needs its own.

### Consumers: everything else

Every other crate carries `#![forbid(unsafe_code)]` in its crate root, and
in the root of each of its integration-test targets, which are separate
crates that the library's attribute does not reach.

`forbid`, not `deny`: `deny` can be switched off again by an inner
`#[allow]`, so it states a preference where this document states a rule.
`forbid` cannot, and rustc rejects the attempt rather than the `unsafe`.

## What `unsafe` is permitted at all

These categories, each with its own bar. A block that fits none of them
does not go in.

1. **A call into a foreign ABI.** Permitted in a `pnpm-ffi-*` crate, and
   only where the safe alternative is absent or wrong. "Absent" means the
   standard library and the workspace's existing dependencies do not offer
   the operation on the target in question. "Wrong" means they offer it and
   it misbehaves there, which is a claim that needs the target named and
   the misbehaviour described: see
   [`pnpm-ffi-fs`](./pnpm/crates/ffi-fs)'s Apple `fsync` fallback and its
   Android `flock` path for the shape of that argument.

2. **A raw-pointer data structure that a safe one cannot replace.** A
   signal handler may not allocate, lock, or unwind, so the registry a
   handler walks cannot be a `Vec` behind a `Mutex`. That constraint earns
   [`pnpm-signal-registry`](./pnpm/crates/signal-registry) and nothing
   wider: the registry is one crate, generic over what it stores, and the
   handlers that walk it hold no `unsafe` of their own.

   Run Miri over a change to one of these. Stacked Borrows rejects
   aliasing no ordinary test can see: deriving the pointer a structure
   stores from a `&mut` is undefined once anything writes through a shared
   reference to the same allocation, and that is easy to write and
   impossible to observe otherwise. The crate's own module docs give the
   invocation, including the flag its deliberate leaks need.

3. **A mutation of process-global state the standard library marks
   unsafe.** `std::env::set_var` and `std::env::remove_var` are unsound
   when another thread reads the environment concurrently, and the reader
   can be inside a library the caller never calls directly. They are
   permitted only behind a value that carries the exclusion:
   `SingleThreaded` in [`pnpm-ffi-process`](./pnpm/crates/ffi-process),
   claimed once at the top of `main` before anything spawns, and
   `EnvGuard` in [`pnpm-testing-utils`](./pnpm/crates/testing-utils),
   which holds a process-wide lock for a test's lifetime. A function that
   takes one of those cannot be called without the proof; no new caller is
   added without it.

## What is never permitted

- `unsafe` in a crate whose purpose is not one of those above. Add the
  call to the abstraction crate that owns the surface, or add a new
  `pnpm-ffi-*` crate for a surface that has no owner yet.
- `unsafe` reached for to silence the borrow checker, to skip a bounds
  check, to transmute between layouts, or to make a type `Send` or `Sync`
  that is not. A measurement, not a hunch, is what reopens any of these,
  and the measurement goes in the pull request.
- A `-sys` declaration written outside a `-sys` crate.
- Widening a `pnpm-ffi-*` crate's public API to a raw handle, pointer, or
  `unsafe fn` so that a caller can do "just a bit more".
- An `#[allow(unsafe_code)]` anywhere. The tier decides, and the tier is
  recorded in the audit's allowlist rather than at a call site.

## `// SAFETY:` comments

Every `unsafe` block carries one, directly above it. It is the obligation
discharged, not a restatement of the call.

It must name **every** precondition the callee states and say what
establishes it here. `// SAFETY: this is fine` and
`// SAFETY: geteuid is always safe to call` are not comments, they are
placeholders: the first says nothing, and the second mistakes "has no
preconditions" for an argument that this call site satisfies them.

The shape that works, for each precondition in turn: what the callee
requires, and the local fact that makes it hold.

```rust
// SAFETY: `name` is NUL-terminated and borrowed for the whole call, the
// parent descriptor is kept open by the `&File`, and the descriptor the
// call returns is adopted by `OwnedFd` before anything can observe it.
```

Two further rules:

- **A claim about another thread, another process, or a signal is the
  precondition most often wrong.** State what serialises the access: a
  lock held across the call, a `Once`, the fact that `main` has not
  started the runtime yet.
- **A claim about the foreign ABI's own behaviour is testable.** Where
  it is not, say so in the pull request instead of asserting it in the
  comment. A claim no test and no documentation backs is not a safety
  argument.

## Adding a foreign call

1. Find the safe alternative first. Check the standard library for the
   target you need, then `[workspace.dependencies]`. A call you can
   delete is worth more than one you can justify.
2. Decide which surface it belongs to and add it to that `pnpm-ffi-*`
   crate. If no crate owns the surface, add one, and keep it to that
   surface.
3. Give it a safe signature: owned returns, translated errors, no raw
   handles out.
4. Write the `// SAFETY:` comment to the bar above.
5. Test it from a consumer crate, where the test cannot reach for
   `unsafe` to set up the fixture. A fixture that needs a foreign call is
   itself a gap in the abstraction.
6. Run `cargo test -p pnpm-unsafe-audit`. It fails if the call landed in a
   crate the policy does not let hold it.

## Enforcement

[`pnpm/tasks/unsafe-audit`](./pnpm/tasks/unsafe-audit) walks every
workspace member and fails when:

- a crate outside its allowlist contains `unsafe`;
- a crate outside its allowlist is missing `#![forbid(unsafe_code)]`, in
  the library root or in an integration-test root;
- a crate whose macros expand to `unsafe` is missing
  `#![deny(unsafe_code)]`, or writes an `unsafe` of its own;
- a crate on the allowlist no longer contains any `unsafe`, so the
  allowlist entry is stale;
- an `unsafe extern` block appears outside a `-sys` crate;
- an `unsafe` block, function or implementation is not preceded by a
  `// SAFETY:` comment. An `unsafe extern` block and an
  `unsafe extern "abi" fn` signature are not: the first states signatures
  and the second defines a callback, so neither discharges an obligation.

The allowlist in that crate is the authoritative list of which crates are
in which tier. This document explains the tiers; the allowlist records the
membership, so the two cannot drift.

What the audit cannot check, a reviewer must: whether a `// SAFETY:`
comment discharges the obligation, and whether a safe alternative was
looked for. See the
[review guide](./.agents/skills/review-code/references/REVIEW_GUIDE.md).
