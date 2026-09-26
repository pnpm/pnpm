# cargo-equivalence

Resolves the same Cargo workspace twice, once with `cargo` and once with
pnpm, against the live crates.io index, and compares what each one locked.

pnpm v12 resolves Cargo dependencies itself rather than shelling out to
`cargo`, so it is a second implementation of `cargo`'s resolver. Being a
drop-in replacement means producing the versions `cargo` produces, not
merely producing a lockfile. Unit tests pin that against synthetic indexes,
which is where the rules are easiest to state and easiest to get subtly
wrong: several resolver bugs have been found by hand-checking a real
workspace against `cargo` after the unit tests were green.

This task is that hand-check, written down and repeatable.

## What a run does

For each workspace in `src/workspaces.rs`:

1. **prepare** — write the manifests into two directories, one per resolver.
2. **cargo** — `cargo generate-lockfile`.
3. **pnpm** — `pnpm install --lockfile-only --no-frozen-lockfile`.
4. **compare** — diff the locked crates, crate by crate.
5. **locked** — drop pnpm's lockfile into a copy of the workspace pnpm never
   touched and run `cargo metadata --locked`. A pristine copy, so this asks
   about the lockfile alone rather than the source replacement pnpm writes
   next to it.

Requirements in the corpus are deliberately open rather than pinned. Both
resolvers read the same index on the same day, so an upstream release
changes what they agree on, not whether they agree.

## Run it

From the repo root, with the Rust CLI built (`cargo build --release --bin
pnpm`) — Cargo support exists only in pnpm v12:

```sh
# Whole corpus
just cargo-equivalence --pnpm ./target/release/pnpm

# One workspace, keeping its lockfiles to inspect
just cargo-equivalence --pnpm ./target/release/pnpm --workspace napi --keep
```

Exit code is non-zero only for an **unexpected** result. Each workspace
lists what it expects:

- `Expectation::Agree` — the resolvers must produce the same crates.
- `Expectation::Differ { issue }` — a known gap, linked to the issue
  tracking it. It reports `KNOWN` and does not fail the run, but it *does*
  fail if the two start agreeing, which is the signal that the issue is
  fixed and the expectation should be raised.

That second state is what keeps a tracked gap from turning the whole run
red, and stops it from being quietly forgotten once it is fixed.

A gap excuses a *disagreement* and nothing else. A workspace that will not
lay out, a resolver that will not run, a lockfile that will not parse — none
of those are a conclusion about what the two resolvers produce, so they fail
the run whatever the workspace expects. Otherwise a tracked gap would be a
blind spot: any regression reaching the same stage would be waved through as
the known difference.

## Adding a workspace

Append a `Workspace` to `WORKSPACES` in `src/workspaces.rs`. A fixture is
only its manifests; every directory whose `Cargo.toml` declares a
`[package]` gets an empty `src/lib.rs` written for it.

Prefer a shape that has already gone wrong. The corpus exists to hold the
cases that unit tests turned out not to cover, so a new entry is worth most
when it comes from a real bug.

## CI

`.github/workflows/cargo-equivalence.yml` runs the corpus on a daily cron.
It is a cron rather than a per-PR check because it resolves against the live
index: an upstream yank or a new release can change a result without
anything in this repo changing, so a red run is something to investigate,
not a merge blocker.
