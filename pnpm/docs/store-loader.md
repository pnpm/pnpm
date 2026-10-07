---
title: Experimental store loader
---

pnpm's experimental loaded linker lets Node.js load JavaScript and JSON directly from the content-addressable store without placing packages in `node_modules`. The loader is included with pnpm.

This is an experimental install mode. Enable it persistently in `pnpm-workspace.yaml` so install and execution commands use the same layout:

```yaml title="pnpm-workspace.yaml"
nodeLinker:
  type: loaded
  excluded:
    - vitest
```

Run `pnpm install`, then use `pnpm run`, `pnpm test`, and `pnpm exec` normally. pnpm generates `node_modules/.pnpm/.store-manifest.json`, writes its bundled runtime to `node_modules/.pnpm/.store-loader.mjs`, and preloads it for scripts and commands. Node child processes inherit the preload through `NODE_OPTIONS`. Generated JavaScript bin shims also register the loader when invoked directly.

`nodeLinker.excluded` contains exact package names. All installed versions and peer contexts of a selected name, plus their complete dependency trees, are materialized as normal GVS packages. Other registry packages stay in CAS. Patched packages, runtimes such as `node@runtime:`, and their dependency trees are materialized automatically. Packages needing build scripts must be selected explicitly unless scripts are disabled. The normal build approval policy still applies.

The mode enables the global virtual store. `node_modules` holds only installation state and bin shims, not packages. GVS packages have their normal dependency links outside the application. Workspace sources remain in their project directories. The project retains its current lockfile and store registration; keep these files while using the installation.

When switching an existing project, remove its old `node_modules` directories before installing. Changing the mode does not remove directories belonging to the previous layout.

The generated files are machine-local. Reinstall after moving the project. CAS installs currently regenerate and verify their runtime manifest on repeat installs rather than using the optimistic installation shortcut.

The loader is embedded in the pnpm executable and needs no separately installed runtime package. The standalone `@pnpm/esm-loader` API remains available for manual integration. Plain `node app.mjs` outside pnpm needs the explicit preload described below.

`symlink: false` and `nodeExperimentalPackageMap` cannot be combined with this mode. Independent test-runner resolvers and direct filesystem reads can still require additional materialization or resolver integration.

## Running an application

The loader requires Node.js 24.18.0 or a later 24.x release, or Node.js 26.2.0 or later. On other versions, every Node.js process that preloads it stops with `ERR_PNPM_LOADER_UNSUPPORTED_NODE`. After a CAS install, run Node directly with:

```sh
node --import ./node_modules/.pnpm/.store-loader.mjs app.mjs
```

For standalone integration, make the loader available separately from the application's dependencies and preload its registration module:

```sh
PNPM_LOADER_MANIFEST=/absolute/path/to/.store-manifest.json \
  node --import /absolute/path/to/esm-loader/register.mjs app.mjs
```

On Windows, set `PNPM_LOADER_MANIFEST` using your shell's environment-variable syntax. If the variable is omitted, the loader reads `node_modules/.pnpm/.store-manifest.json` from the current working directory.

A custom preload can register an explicit manifest:

```js
import { registerStoreLoader } from '@pnpm/esm-loader'

registerStoreLoader(new URL('./node_modules/.pnpm/.store-manifest.json', import.meta.url))
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

## Opting out packages into the global virtual store

A package that needs physical files can use a normal global virtual store (GVS) installation, including its full locked dependency tree and dependency links. The application's `node_modules` does not need to contain it. GVS itself retains its conventional `node_modules` layout.

Point the package's manifest entry at its real GVS directory and set `resolution` to `node`:

```json
{
  "root": "/path/to/store/links/@/tool/1.0.0/<graph-hash>/node_modules/tool",
  "resolution": "node"
}
```

The application's dependency map still points to this package's instance ID. Imports originating inside the opted-out package use Node's normal resolution and its physical dependency links. The entry does not need a loader dependency map. Explicit paths to CAS modules remain loadable through the hooks. Map materialized transitive package instances to their GVS roots too, so application imports of those instances use the same modules. Multiple opted-out IDs can share a GVS root when pnpm deduplicates them.

Use pnpm's normal installer to populate GVS; copying just the selected package's files does not create a complete opt-out. The installer owns graph hashes, package import, linking, build policy, and store registration. Retain the installation that references those entries so store pruning can track their usage. The standalone loader does not install packages or register its manifest with store pruning. Normal CAS installs register the project, and `pnpm store prune` preserves both manifest-referenced blobs and materialized dependency trees.

With `frozenStore`, pnpm does not register the project in the read-only store. The store must already be populated, and its owner must retain the files the project needs.

This is an explicit compatibility fallback, not an install-time static analyzer. It can materialize a large dependency tree, but ordinary GVS entries can be reused across projects. Packages outside that tree stay in CAS. A tool that directly reads application dependencies outside its own tree may still need additional opt-outs or resolver integration.

## Supported behavior

The loader supports ESM and CommonJS, relative modules, dynamic imports, `createRequire`, JSON imports, package `exports` and `imports`, conditional exports, export patterns, package self references, aliases, and workspace dependency contexts. Resolution uses `enhanced-resolve` with Node-oriented settings.

Stored modules have virtual file URLs under `.pnpm-loader/` next to the manifest. That directory is never created. These URLs retain package identity and source filenames; the loader reads the corresponding bytes from CAS and verifies their hashes. The virtual namespace is reserved and cannot also be a workspace root.

Bare imports resolve to declared dependencies. A dependency bundled in the importing package's own `node_modules` directory takes precedence, as it does in Node.js. There is no fallback to an application's existing `node_modules` tree for a dependency missing from the manifest. Built-in modules continue to use Node's loader. Absolute paths returned by resolvers remain loadable. Relative local application files use Node's normal resolution. Packages without a `package.json` receive an in-memory empty manifest to keep resolution within their package boundary.

## Compatibility limits

This loader does not emulate a general filesystem. `import.meta.url`, `__filename`, `__dirname`, and `require.resolve()` for stored modules identify virtual locations. Passing them to ordinary `fs` APIs will not read store assets. Packages that read adjacent assets, scan directories, write into their package directory, or depend on a physical filename need materialization or a separate filesystem integration.

Native addons, WebAssembly, TypeScript, and package-internal symlinks are not supported by this runtime. JavaScript bins may have no file extension. Native addons may also require neighboring shared libraries, even if their build output is cached.

Use `.mjs` or `"type": "module"` for ESM. Stored `.js` files without a package type are treated as CommonJS; Node's syntax detection for ambiguous `.js` files is not implemented. The resolver is not a promise of complete Node resolution parity.

The loader does not run lifecycle scripts, select side-effects cache entries, create command shims, or generate manifests from lockfiles. A package with install scripts can run from stored build outputs if its effective files otherwise meet these compatibility requirements. Runtime and development dependencies have the same filesystem constraints.

The manifest is trusted configuration, as a lockfile and preload script are. Dependency restrictions and blob verification do not sandbox package code.
