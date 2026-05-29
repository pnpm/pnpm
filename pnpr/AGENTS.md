# AGENTS.md (pnpr)

Guidance for AI coding agents working in `pnpr/`.

**Read [`../AGENTS.md`](../AGENTS.md) first.** It covers the monorepo-wide
conventions: GitHub PR workflow, signing agent-authored content, conventional
commit messages, code-reuse philosophy, and "never ignore test failures."

## What this project is

`pnpr/` is a pnpm-compatible npm registry server written in Rust —
roughly the role [verdaccio](https://verdaccio.org/) plays in the JS
ecosystem. It is a **sibling** of `pacquet/`, not part of it.

The two Rust projects share the same Cargo workspace at the repo root so
that the registry can depend directly on `pacquet-*` crates (tarball
handling, integrity hashes, manifest parsing, network plumbing, etc.) and
the `Cargo.lock` stays unified.

## Relationship to pacquet

- **`pacquet/`** is a *port* of the pnpm CLI. Its cardinal rule is
  "match pnpm exactly" — see [`../pacquet/AGENTS.md`](../pacquet/AGENTS.md).
- **`pnpr/`** has no pnpm-CLI counterpart to mirror. It is a new
  server. Behavior here is designed, not ported.

That means the "match upstream pnpm" discipline that governs `pacquet/`
does **not** apply here. The registry can pick its own architecture,
flags, and config format. It must still be compatible with the npm
registry protocol that pnpm (and npm, yarn, etc.) clients speak.

## Layout

Mirrors `pacquet/`:

```text
pnpr/
  AGENTS.md
  crates/
    pnpr/          -> package "pnpr"
      Cargo.toml
      README.md
      src/
        lib.rs              -> library API
        main.rs             -> binary entry point (ships the `pnpr` binary)
    # future sibling crates land here, see "New registry-only crates" below
```

The Rust workspace itself, `rust-toolchain.toml`, `justfile`, and
`Cargo.lock` live at the **repo root** — run `cargo` and `just` from there.
`pnpr/crates/*` is wired into the root workspace `members`.

## Code reuse

**Prefer existing `pacquet-*` crates over writing new code.** Before
implementing anything non-trivial, check whether `pacquet-*` already
solves it. Candidates worth checking first: `pacquet-tarball`,
`pacquet-crypto-hash`, `pacquet-crypto-shasums-file`,
`pacquet-package-manifest`, `pacquet-network`, `pacquet-registry`,
`pacquet-fs`, `pacquet-diagnostics`. Add a `pacquet-*` crate the same
way pacquet crates do: declare it in the root `[workspace.dependencies]`
(already done for the pacquet crates) and use `{ workspace = true }`
in this crate's `Cargo.toml`.

If a piece of code currently inside `pacquet/` turns out to be genuinely
shared between the two stacks and the `pacquet-` prefix becomes
misleading, propose renaming/relocating it in a dedicated PR — not as a
drive-by during feature work.

### New registry-only crates

When the registry needs its own crate (logic that isn't shared with the
pnpm port and doesn't fit in `pacquet/`), put it under
`pnpr/crates/<short-name>/` and name the package
`pnpr-<short-name>` in its `Cargo.toml`. The
`pnpr/crates/*` glob in the root workspace `members` picks it up
automatically; just add the new crate to `[workspace.dependencies]` at
the root with the `pnpr-` prefix so other crates can use
`{ workspace = true }`.

Use the `pnpr-` prefix exclusively for registry-only crates.
Don't reach for `pacquet-` to name something new on the registry side.

## Dependencies

Same rule as pacquet: a dependency that is already declared in
`[workspace.dependencies]` may be used by any crate that needs it.
Adding a new third-party crate to the workspace requires an explicit
human request (see [`../pacquet/AGENTS.md`](../pacquet/AGENTS.md#things-not-to-do)).

## Style, tests, commits

Follow the pacquet code-style guide
([`../pacquet/CODE_STYLE_GUIDE.md`](../pacquet/CODE_STYLE_GUIDE.md)) and the
pacquet contributing guide ([`../pacquet/CONTRIBUTING.md`](../pacquet/CONTRIBUTING.md))
for Rust-level conventions — imports, naming, ownership, error handling,
test layout. They are written for pacquet but apply to any Rust code in
this workspace.

Commit messages use Conventional Commits with `pnpr` as the scope
(`feat(pnpr): ...`, `fix(pnpr): ...`).

Run the same checks pacquet does before declaring work done:

```sh
just check     # cargo check --locked --workspace --all-targets
just test      # cargo nextest run
just lint      # cargo clippy --workspace --all-targets -- --deny warnings
just fmt       # cargo fmt + taplo format
```
