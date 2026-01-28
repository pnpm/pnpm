# Bundling pnpm with Deno

This document explains how to compile pnpm into a standalone single executable using [Deno](https://deno.com/).

## Prerequisites

- **Deno** >= 2.6 (earlier versions may segfault)
- **pnpm** must be compiled first (`pnpm run compile` in the `pnpm/` directory), which produces `dist/pnpm.mjs` and `dist/worker.js`

## Quick Start

```bash
cd pnpm/

# Step 1: Build pnpm (tsgo + esbuild bundle)
pnpm run compile

# Step 2: Compile to standalone binary
pnpm run deno:compile
# or directly:
deno run -A deno-compile.ts

# Step 3: Test
./pnpm-deno --version
```

The output binary is `pnpm-deno` (~78 MB).

## How It Works

The `deno-compile.ts` script performs the following steps:

### 1. Patch Node.js imports

esbuild produces bundles with bare Node.js specifiers (e.g., `from "fs"`), but Deno requires the `node:` prefix (e.g., `from "node:fs"`). The script patches both `dist/pnpm.mjs` and `dist/worker.js` to add the prefix for all Node.js built-in modules. Results are written to a temporary `_deno_patched/` directory.

### 2. Embed the worker

pnpm uses `@rushstack/worker-pool` with Node.js `worker_threads`. The worker script must be a file on disk. The compile script:

1. Reads the patched `worker.js` source
2. Embeds it as a string literal in the generated entry point
3. At runtime, extracts it to a temp directory (`/tmp/pnpm-deno-<pid>/worker.js`)
4. Sets `process.env.PNPM_WORKER_PATH` so the worker pool finds it

### 3. Embed native addons

If any `.node` files exist in `dist/` (e.g., `@reflink/reflink`), they are base64-encoded and embedded in the entry point, then extracted to the same temp directory at runtime.

### 4. Generate entry point

A thin `_deno_entry.mjs` file is generated that:

- Shims `globalThis.global` (Deno doesn't set `global` like Node.js)
- Shims `globalThis.Buffer` (not a global in Deno)
- Shims `globalThis.require` via `createRequire()` (needed for `eval('require')` in `@yarnpkg/fslib`)
- Extracts the embedded worker to disk
- Sets `PNPM_WORKER_PATH` environment variable
- Imports the patched `pnpm.mjs` bundle
- Cleans up temp files on process exit

### 5. Compile

Runs `deno compile --allow-all --node-modules-dir=auto` to produce the standalone binary.

## Architecture

```
deno-compile.ts
├── Reads dist/pnpm.mjs and dist/worker.js (from pnpm's existing build)
├── Patches bare Node.js imports → node: prefixed
├── Generates _deno_entry.mjs with:
│   ├── Global shims (global, Buffer, require)
│   ├── Embedded worker.js (string literal)
│   ├── Embedded .node addons (base64)
│   └── import('./_deno_patched/pnpm.mjs')
└── Runs: deno compile --allow-all --output=pnpm-deno _deno_entry.mjs
```

## Key Compatibility Shims

### `globalThis.global`

Deno doesn't define `global` like Node.js does. Many npm packages reference `global`, so we set:

```js
if (typeof globalThis.global === 'undefined') globalThis.global = globalThis;
```

### `globalThis.Buffer`

`Buffer` is not a global in Deno. We import it from `node:buffer` and expose it:

```js
import { Buffer } from 'node:buffer';
if (typeof globalThis.Buffer === 'undefined') globalThis.Buffer = Buffer;
```

### `globalThis.require`

`@yarnpkg/fslib` uses `eval('require')` for CJS interop. In Deno's ESM environment, `require` is not defined. We create one via `createRequire`:

```js
import { createRequire } from 'node:module';
globalThis.require = createRequire(import.meta.url);
```

### Bare Node.js import prefix

esbuild bundles contain bare specifiers like `from "fs"` or `require("path")`. Deno requires the `node:` prefix for all Node.js built-ins. The `addNodePrefixes()` function handles this for 50+ modules.

## Worker Path Override

The `PNPM_WORKER_PATH` environment variable tells pnpm's worker pool where to find `worker.js`. This is set in `worker/src/index.ts`:

```typescript
workerScriptPath: process.env.PNPM_WORKER_PATH ?? path.join(import.meta.dirname, 'worker.js'),
```

This is essential for compiled binaries where the worker can't be found relative to the bundle.

## Limitations

- **Experimental**: Deno's Node.js compatibility is good but not 100%. Some edge cases may fail.
- **Binary size**: ~78 MB (Deno runtime is embedded). The original JS bundle is ~8 MB and requires a separate Node.js installation.
- **No native reflink**: The `@reflink/reflink` `.node` addon may not load correctly in Deno's compiled binary. pnpm falls back to `fs.copyFileSync`.
- **Deno >= 2.6 required**: Earlier versions (e.g., 2.0.x) may segfault during compilation or at runtime.
- **Platform-specific**: `deno compile` produces a binary for the current platform. Cross-compilation is possible with `--target` (e.g., `--target x86_64-unknown-linux-gnu`).
- **Temp file cleanup**: The worker and native addons are extracted to `/tmp/pnpm-deno-<pid>/` at startup and cleaned up on exit. Abnormal termination may leave temp files behind.

## Cross-Compilation

Deno supports cross-compilation via the `--target` flag. To use it, modify `deno-compile.ts` or run `deno compile` manually:

```bash
# Linux x86_64
deno compile --allow-all --target x86_64-unknown-linux-gnu --output pnpm-deno-linux _deno_entry.mjs

# Windows x86_64
deno compile --allow-all --target x86_64-pc-windows-msvc --output pnpm-deno.exe _deno_entry.mjs

# macOS ARM
deno compile --allow-all --target aarch64-apple-darwin --output pnpm-deno-arm _deno_entry.mjs
```

## Verified Commands

The following commands have been tested with the Deno-compiled binary:

- `pnpm-deno --version`
- `pnpm-deno --help`
- `pnpm-deno init`
- `pnpm-deno add <package>`
- `pnpm-deno remove <package>`
- `pnpm-deno list`
