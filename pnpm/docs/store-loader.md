---
title: Experimental store loader
---

The standalone `@pnpm/esm-loader` package lets Node.js load JavaScript and JSON directly from pnpm's content-addressable store. It reads a dependency manifest and store blobs without writing package directories or a `node_modules` directory for the application.

This is an experimental runtime API. `pnpm install` does not yet generate its manifest or select this loader. Integration with an opt-in install mode, bin shims, and selective materialization is planned separately.

## Running an application

The loader requires Node.js 22.15.0 or later in the 22.x release line, or Node.js 24 or later. It uses synchronous module hooks. Make the loader available separately from the application's dependencies, then preload its registration module:

```sh
PNPM_LOADER_MANIFEST=/absolute/path/to/.pnpm-store.json \
  node --import /absolute/path/to/esm-loader/register.mjs app.mjs
```

On Windows, set `PNPM_LOADER_MANIFEST` using your shell's environment-variable syntax. If the variable is omitted, the loader reads `.pnpm-store.json` from the current working directory.

A custom preload can register an explicit manifest:

```js
import { registerStoreLoader } from '@pnpm/esm-loader'

registerStoreLoader(new URL('./.pnpm-store.json', import.meta.url))
```

Run it with `node --import ./preload.mjs app.mjs`. The registration returns Node's hook handle, which has a `deregister()` method. Register before importing application dependencies. Workers inherit a `--import` preload through Node's default execution arguments.

## Manifest format

The manifest has three top-level fields:

| Field | Meaning |
| --- | --- |
| `version` | `1` |
| `storeDir` | The versioned store directory containing `files/`. Relative paths resolve from the manifest's directory. |
| `packages` | An object keyed by package instance ID, including each peer dependency context. |

Each package entry has a `dependencies` object mapping import names to package instance IDs, and exactly one of:

- `root`: a real project or workspace directory, relative to the manifest or absolute.
- `files`: an object mapping package-relative filenames to their lowercase SHA-512 hex digest, with `-exec` appended for executable blobs.

For example, the following structure describes a project importing a stored package. Replace the digest placeholders with the actual 128-character SHA-512 hashes:

```json
{
  "version": 1,
  "storeDir": "/home/user/.local/share/pnpm/store/v11",
  "packages": {
    ".": {
      "root": ".",
      "dependencies": { "example": "example@1.0.0" }
    },
    "example@1.0.0": {
      "dependencies": {},
      "files": {
        "package.json": "<sha512 hex digest>",
        "index.js": "<sha512 hex digest>"
      }
    }
  }
}
```

A file with digest `abcdef...` is read from `<storeDir>/files/ab/cdef...`. No package metadata is inferred from the blob filename. Include `package.json` in the file map to supply `type`, `main`, `exports`, and `imports`.

Use separate package IDs for separate dependency contexts, even when every file digest is identical. For example, `plugin@1.0.0(peer@1.0.0)` and `plugin@1.0.0(peer@2.0.0)` can share their `files` map while having different dependency targets. Their runtime module instances remain separate.

The manifest describes the effective files to load. A producer can represent cached build outputs by applying a side-effects cache diff to the base index, including deletions and replacements, before generating this file map. Selecting a valid cache entry and authorizing builds remain the installer's responsibility.

## Supported behavior

The loader supports ESM and CommonJS, relative modules, dynamic imports, `createRequire`, JSON imports, package `exports` and `imports`, conditional exports, export patterns, package self references, aliases, and workspace dependency contexts. Resolution uses `enhanced-resolve` with Node-oriented settings.

Stored modules have virtual file URLs under `.pnpm-loader/` next to the manifest. That directory is never created. These URLs retain package identity and source filenames; the loader reads the corresponding bytes from CAS and verifies their hashes. The virtual namespace is reserved and cannot also be a workspace root.

Only declared dependencies are available through bare imports. There is no fallback to an application's existing `node_modules` tree for a dependency missing from the manifest. Built-in modules continue to use Node's loader. Relative local application files use Node's normal resolution.

## Compatibility limits

This loader does not emulate a general filesystem. `import.meta.url`, `__filename`, `__dirname`, and `require.resolve()` for stored modules identify virtual locations. Passing them to ordinary `fs` APIs will not read store assets. Packages that read adjacent assets, scan directories, write into their package directory, or depend on a physical filename need materialization or a separate filesystem integration.

Native addons, WebAssembly, TypeScript, extensionless source files, bundled `node_modules` directories, and package-internal symlinks are not supported by this initial runtime. Native addons may also require neighboring shared libraries, even if their build output is cached.

Use `.mjs` or `"type": "module"` for ESM. Stored `.js` files without a package type are treated as CommonJS; Node's syntax detection for ambiguous `.js` files is not implemented. The resolver is not a promise of complete Node resolution parity.

The loader does not run lifecycle scripts, select side-effects cache entries, create command shims, or generate manifests from lockfiles. A package with install scripts can run from stored build outputs if its effective files otherwise meet these compatibility requirements. Runtime and development dependencies have the same filesystem constraints.

The manifest is trusted configuration, as a lockfile and preload script are. Dependency restrictions and blob verification do not sandbox package code.
