# ecosystem-e2e

Installs real-world JavaScript stacks with both the pnpm CLI and pacquet,
across both `node_modules` layouts, then builds each app to prove the
produced layout actually works.

It exists for two reasons specific to this repo:

- **pacquet parity** — pacquet and the pnpm CLI are parallel implementations
  kept at parity. A real framework that installs and builds under pnpm but not
  under pacquet is a far better parity signal than any unit test.
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
# Whole grid, every stack (pnpm must be built: cargo build --release --bin pnpm)
cargo run -p pnpm-ecosystem-e2e -- --pacquet ./target/release/pnpm

# Just pnpm, one stack, both layouts
cargo run -p pnpm-ecosystem-e2e -- --binary pnpm --stack vite-react

# Iterate without re-scaffolding
cargo run -p pnpm-ecosystem-e2e -- --stack vite-react --keep
```

Exit code is non-zero if any cell fails. Per-cell logs are written to
`<work-dir>/cells/<cell-id>/cell.log`.

## Adding a stack

Append a `Stack` to `STACKS` in `src/stacks.rs`. Pin the generator to a major
version — an unpinned `@latest` turns an upstream framework release into a red
cell that looks like a pnpm/pacquet regression. Bump pins deliberately.

## WebContainers

`webcontainer/` installs a small project with the pnpm CLI bundle inside a
StackBlitz WebContainer, booted in headless Chromium. WebContainers run
Node.js in the browser with their own `fs` and `node:sqlite`, which differ from
Node.js in ways no local test reproduces. The default mode tests the TypeScript
CLI bundle: install without a lockfile, repeat install, offline reinstall from
the store, `add`, `remove`, and `list`.

From the repo root, with the bundle built (`pnpm --filter pnpm run compile`):

```sh
pnpm --filter @pnpm-private/ecosystem-e2e-webcontainer exec playwright-core install --only-shell chromium
node pnpm/tasks/ecosystem-e2e/webcontainer/run.mjs
```

The Rust CLI runs through the WebAssembly runtime. Test the `@pnpm/wasm`
package with `--wasm-package`. This mode installs the package locally and
globally with npm, exercises all four bin aliases and local `npx` dispatch,
then runs the CLI workflows:

```sh
pnpm pack:pnpm:wasm
node pnpm/tasks/ecosystem-e2e/webcontainer/run.mjs --wasm-package target/pnpm-wasm.tgz
```

The workflows cover update, lockfile-only and frozen/offline installs,
configuration, command failures, `dlx`, `create`, workspace scripts and bins,
interactive build approval, rebuild, pack, and a Vite build. Expected failures
assert their exit status and diagnostic. These are representative workflow
tests, not a claim that every command or native dependency works in a browser.

### Rust WASM host probe

The v12 WebContainer port is tracked in
[pnpm/tasks#63](https://github.com/pnpm/tasks/issues/63). The capability probe
runs a Rust WASI module and Node host checks in a real WebContainer without
building or running the TypeScript CLI:

```sh
rustup target add wasm32-wasip1
rustc --target wasm32-wasip1 pnpm/tasks/ecosystem-e2e/webcontainer/wasm-probe.rs -o /tmp/pnpm-wasm-probe.wasm
node pnpm/tasks/ecosystem-e2e/webcontainer/run.mjs --wasm-probe /tmp/pnpm-wasm-probe.wasm
```

It asserts WASI file reads/writes, rename and hardlinks, and checks that Node
sees the same files. It also asserts Node symlinks, randomness, workers,
child process exit status and registry HTTP streaming. Rust thread/process
results and missing Node SQLite methods are reported as capability evidence;
their absence does not fail the probe. When the SQLite methods exist, the
probe verifies persistence across reopening the database.

A successful probe does **not** mean pnpm v12 can install packages in
WebContainers. It establishes the host capabilities needed to implement that
port. Native CLI code and build configuration are unaffected.

The threaded runtime probe exercises Tokio, Rayon, cross-thread file
descriptors and the Rust-to-Node HTTP/process bridge. Build it before testing:

```sh
rustup target add wasm32-wasip1-threads
pnpm build:wasm-runtime-probe
node pnpm/tasks/ecosystem-e2e/webcontainer/run.mjs --wasm-runtime target/wasm-runtime-probe/wasm32-wasip1-threads/debug/pnpm-wasm-runtime-probe.wasm
```

The [runtime guide](../../wasm/README.md) describes its host contract and
validation boundaries.

## CI

`.github/workflows/ecosystem-e2e.yml` runs the grid on a daily cron (one job
per stack) against this repo's built pnpm bundle and a freshly built pacquet.
A red cell is something to investigate, not a merge blocker — hence cron, not
per-PR.
The `webcontainer` job in the same workflow runs the WebContainer test
against the same bundle.
