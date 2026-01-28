# Bundling pnpm with Bun

This document describes how to compile pnpm into a single standalone executable using [Bun](https://bun.sh/).

> **Status: Experimental** — Bun's Node.js compatibility is good but not 100%. Some pnpm features may not work correctly.

## Prerequisites

- [Bun](https://bun.sh/) v1.1+ installed
- pnpm workspace dependencies installed (`pnpm install`)
- TypeScript sources compiled (`pnpm run _compile` from the `pnpm/` directory, or `pnpm run compile` from the root)

## Quick Start

From the `pnpm/` directory:

```bash
# 1. Compile TypeScript to lib/
pnpm run _compile

# 2. Compile to a single executable
bun run bun-compile.ts
```

This produces a `pnpm-bun` binary (~65 MB) in the `pnpm/` directory.

```bash
./pnpm-bun --version
./pnpm-bun install
```

## Scripts

Two npm scripts are available in `pnpm/package.json`:

| Script | Command | Description |
|--------|---------|-------------|
| `bun:bundle` | `bun run bun-bundle.ts` | Bundles pnpm to `dist/` using Bun (replaces esbuild) |
| `bun:compile` | `bun run bun-compile.ts` | Compiles pnpm into a standalone binary |

### `bun:bundle`

Produces `dist/pnpm.mjs` and `dist/worker.js` using Bun's bundler API, mirroring what `bundle.ts` does with esbuild. Also copies static assets (node-gyp-bin, templates, vendor, pnpmrc). Use this if you want a Bun-bundled JS distribution that still runs on Node.js.

### `bun:compile`

Produces a self-contained `pnpm-bun` binary. This:

1. Bundles the worker thread code separately via `Bun.build()`
2. Generates an entry point that embeds the worker as a string literal
3. Runs `bun build --compile` to produce the final binary

The `bun:bundle` step is **not** required before `bun:compile` — the compile script bundles directly from the TypeScript-compiled `lib/` sources.

## How It Works

### Architecture

```
pnpm/lib/pnpm.js  (compiled TypeScript)
       │
       ├── bun-compile.ts bundles worker separately via Bun.build()
       │   └── worker source embedded as string in entry point
       │
       ├── entry point extracts worker.js to /tmp at startup
       │   └── sets PNPM_WORKER_PATH env var
       │
       └── bun build --compile bundles 2477 modules into single binary
           └── pnpm-bun (~65 MB)
```

### Worker Thread Handling

pnpm uses `@rushstack/worker-pool` to run tarball extraction and linking in worker threads. Workers require a standalone `.js` file on disk. The compile script:

1. Pre-bundles `worker/lib/worker.js` into a single file
2. Embeds the worker source as a string constant in the entry point
3. At startup, writes the worker to `/tmp/pnpm-bun-{pid}/worker.js`
4. Sets `PNPM_WORKER_PATH` so the worker pool loads from there
5. Cleans up the temp directory on process exit

### The `require` Shim

`@yarnpkg/fslib` uses `eval('require')` to obtain a CJS `require` reference that survives bundlers. In Bun compiled binaries, module-scoped `const require` gets scoped away by the bundler. The fix is:

```js
globalThis.require = createRequire(import.meta.url);
```

This persists across all scopes and is accessible to `eval()`.

## Cross-Compilation

Bun supports cross-compilation via `--target`:

```bash
bun build --compile --target=bun-linux-x64 entry.mjs --outfile pnpm-linux
bun build --compile --target=bun-linux-arm64 entry.mjs --outfile pnpm-linux-arm64
bun build --compile --target=bun-windows-x64 entry.mjs --outfile pnpm.exe
```

To use cross-compilation, modify `bun-compile.ts` to pass the target flag, or run the CLI `bun build --compile` manually on the generated entry file.

## Limitations

- **Binary size**: ~65 MB (includes Bun runtime) vs ~8 MB JS bundle + Node.js
- **Native addons**: The `@reflink/reflink` `.node` file cannot be embedded. pnpm falls back to `fs.copyFileSync` with `COPYFILE_FICLONE_FORCE` when reflink is unavailable.
- **Node.js compatibility**: Bun implements most Node.js APIs but not all. Edge cases in lifecycle scripts, `node-gyp`, or network code may behave differently.
- **Worker threads**: Uses Bun's `worker_threads` compatibility layer. Tested and working, but edge cases may exist.
- **Platform-specific**: Each binary is compiled for a single platform/arch.
