# ecosystem-e2e

Installs real-world JavaScript stacks with both the pnpm CLI and pacquet,
across both `node_modules` layouts, then builds each app to prove the
produced layout actually works.

It exists for two reasons specific to this repo:

- **pacquet parity** — pacquet is a Rust port of pnpm. A real framework that
  installs and builds under pnpm but not under pacquet is a far better parity
  signal than any unit test.
- **global virtual store** — the global virtual store relocates where
  dependencies physically live, the same class of change that forced Yarn to
  build ecosystem tests for Plug'n'Play. Running real stacks under it is the
  only way to find tools that break on the new layout.

## The grid

Every run is the cross product of three axes:

```text
binary:  pnpm | pacquet            (--binary)
layout:  isolated | global-virtual-store   (--layout)
stack:   next | vite-react | ...   (--stack, defaults to all)
```

Each cell runs four stages, stopping at the first failure:

1. **prepare** — scaffold the project once (with `pnpm dlx`, no install), copy
   it into the cell, write a `pnpm-workspace.yaml` pinning an isolated
   store/cache and the layout.
2. **install** — `<binary> install`.
3. **build** — run the project's build script with `node_modules/.bin` on
   `PATH`, no package manager involved, so the build can't re-install and mask
   the install under test. Proves the layout resolves at **bundle time**.
4. **serve** — boot the production server, poll it over HTTP until it answers,
   and require a non-error (`2xx`/`3xx`) response, then kill it. Proves the
   layout works at **runtime** — request-time `require`, SSR, native addons —
   which a build alone does not exercise. Stacks without a server skip this
   stage; `--skip-serve` skips it everywhere for quick iteration.

## Run it

From the repo root:

```sh
# Whole grid, every stack (pacquet must be built: cargo build --release --bin pacquet)
cargo run -p pacquet-ecosystem-e2e -- --pacquet ./target/release/pacquet

# Just pnpm, one stack, both layouts
cargo run -p pacquet-ecosystem-e2e -- --binary pnpm --stack vite-react

# Iterate without re-scaffolding
cargo run -p pacquet-ecosystem-e2e -- --stack vite-react --keep
```

Exit code is non-zero if any cell fails. Per-cell logs are written to
`<work-dir>/cells/<cell-id>/cell.log`.

## Adding a stack

Append a `Stack` to `STACKS` in `src/stacks.rs`. Pin the generator to a major
version — an unpinned `@latest` turns an upstream framework release into a red
cell that looks like a pnpm/pacquet regression. Bump pins deliberately.

## CI

`.github/workflows/ecosystem-e2e.yml` runs the grid on a daily cron (one job
per stack) against this repo's built pnpm bundle and a freshly built pacquet.
A red cell is something to investigate, not a merge blocker — hence cron, not
per-PR.
