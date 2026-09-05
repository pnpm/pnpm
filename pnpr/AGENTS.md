# AGENTS.md (pnpr)

Guidance for AI coding agents working in `pnpr/`.

**Read [`../AGENTS.md`](../AGENTS.md) first.** It covers the monorepo-wide
conventions: GitHub PR workflow, signing agent-authored content, conventional
commit messages, code-reuse philosophy, and "never ignore test failures."

## What this project is

`pnpr/` is a pnpm-compatible npm registry server written in Rust —
roughly the role [verdaccio](https://verdaccio.org/) plays in the JS
ecosystem. It is a **sibling** of `pnpm/`, not part of it.

The two Rust projects share the same Cargo workspace at the repo root so
that the registry can depend directly on the pacquet `pnpm-*` crates (tarball
handling, integrity hashes, manifest parsing, network plumbing, etc.) and
the `Cargo.lock` stays unified.

## Relationship to pacquet

- **`pnpm/`** is pnpm v12 and the target for new feature development. See
  [`../pnpm/AGENTS.md`](../pnpm/AGENTS.md) for its version policy.
- **`pnpr/`** has no pnpm-CLI counterpart to mirror. It is a new
  server. Behavior here is designed, not ported.

The pnpm v12 and v11 coverage policy governs the CLI implementations, not
pnpr. The registry can pick its own architecture, flags, and config format. It
must still be compatible with the npm registry protocol that pnpm (and npm,
yarn, etc.) clients speak.

## Layout

Mirrors `pnpm/`:

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
    auth/          -> package "pnpr-auth"          (user and token stores)
    config/        -> package "pnpr-config"        (the YAML config: parsing and validation)
    error/         -> package "pnpr-error"         (the error type every layer returns)
    osv/           -> package "pnpr-osv"           (the OSV advisory index)
    package-name/  -> package "pnpr-package-name"  (npm package-name parsing)
    registry/      -> package "pnpr-registry"      (the registry routing table)
    route/         -> package "pnpr-route"         (classifies a fetch route public or private)
    search/        -> package "pnpr-search"        (the local /-/v1/search index scan)
    shared-artifacts/ -> package "pnpr-shared-artifacts" (the shared build-artifact store)
    storage/       -> package "pnpr-storage"       (the hosted store, proxy cache, and publish journal)
    upstream/      -> package "pnpr-upstream"      (the upstream registry proxy client)
    policy/        -> package "pnpr-policy"        (access policy for those routes)
    # further sibling crates land here, see "New registry-only crates" below
```

The Rust workspace itself, `rust-toolchain.toml`, `justfile`, and
`Cargo.lock` live at the **repo root** — run `cargo` and `just` from there.
`pnpr/crates/*` is wired into the root workspace `members`.

## Code reuse

**Prefer existing `pnpm-*` crates over writing new code.** Before
implementing anything non-trivial, check whether `pnpm-*` already
solves it. Candidates worth checking first: `pnpm-tarball`,
`pnpm-crypto-hash`, `pnpm-crypto-shasums-file`,
`pnpm-package-manifest`, `pnpm-network`, `pnpm-registry`,
`pnpm-fs`, `pnpm-diagnostics`. Add a `pnpm-*` crate the same
way pacquet crates do: declare it in the root `[workspace.dependencies]`
(already done for the pacquet crates) and use `{ workspace = true }`
in this crate's `Cargo.toml`.

If a piece of code currently inside `pnpm/` turns out to be genuinely
shared between the two stacks and living under `pnpm/crates/` becomes
misleading, propose renaming/relocating it in a dedicated PR — not as a
drive-by during feature work.

### New registry-only crates

When the registry needs its own crate (logic that isn't shared with the
pnpm CLI and doesn't fit in `pnpm/`), put it under
`pnpr/crates/<short-name>/` and name the package
`pnpr-<short-name>` in its `Cargo.toml`. The
`pnpr/crates/*` glob in the root workspace `members` picks it up
automatically; just add the new crate to `[workspace.dependencies]` at
the root with the `pnpr-` prefix so other crates can use
`{ workspace = true }`.

Use the `pnpr-` prefix exclusively for registry-only crates.
Don't reach for `pnpm-` to name something new on the registry side.

**Also add it to `deny.toml`.** The `pnpr*` crates are licensed under
PolyForm Shield rather than the workspace's MIT, and cargo-deny scores that
license text below its confidence threshold. A new crate therefore needs both
an `licenses.exceptions` entry and a `[[licenses.clarify]]` block pinning
`../../LICENSE.md` by hash — without them `Rust CI / Cargo Deny` fails the
crate as `unlicensed`. That job is path-filtered on `Cargo.lock` and
`deny.toml`, so it only runs on PRs that touch one of them; adding a crate
always does.

Name crates for what they hold, not for where they sit in the graph. The
workspace has no `core`, `common`, `shared`, or `utils` crate, and shouldn't
grow one: those names admit anything, so they collect everything.

## Dependencies

Same rule as pacquet: a dependency that is already declared in
`[workspace.dependencies]` may be used by any crate that needs it.
Adding a new third-party crate to the workspace requires an explicit
human request (see [`../pnpm/AGENTS.md`](../pnpm/AGENTS.md#things-not-to-do)).

## Style, tests, commits

Follow the pacquet code-style guide
([`../pnpm/CODE_STYLE_GUIDE.md`](../pnpm/CODE_STYLE_GUIDE.md)) and the
pacquet contributing guide ([`../pnpm/CONTRIBUTING.md`](../pnpm/CONTRIBUTING.md))
for Rust-level conventions — imports, naming, ownership, error handling,
test layout. They are written for pacquet but apply to any Rust code in
this workspace.

### Comments

Follow the repo-wide comment baseline in [`../AGENTS.md`](../AGENTS.md#comments) and the Rust-specific additions in [`../pnpm/AGENTS.md`](../pnpm/AGENTS.md#comments).

Commit messages use Conventional Commits with `pnpr` as the scope
(`feat(pnpr): ...`, `fix(pnpr): ...`).

Run the same checks pacquet does before declaring work done:

```sh
just check     # cargo check --locked --workspace --all-targets
just test      # cargo nextest run
just lint      # cargo clippy --workspace --all-targets -- --deny warnings
just fmt       # cargo fmt + taplo format
```
