---
title: Experimental store loader
---

The standalone `@pnpm/esm-loader` package lets Node.js load JavaScript and JSON directly from pnpm's content-addressable store. It reads a dependency manifest and store blobs without writing package directories or a `node_modules` directory for the application.

This is an experimental runtime API. `pnpm install` does not yet generate its manifest or select this loader. Integration with an opt-in install mode and bin shims is planned separately. A standalone diagnostic can install explicitly opted-out packages and their dependency trees into the global virtual store.

## Running an application

The loader requires Node.js 26.10.0 or later. It uses synchronous module hooks. Make the loader available separately from the application's dependencies, then preload its registration module:

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

## Opting out packages into the global virtual store

A package that needs physical files can use a normal global virtual store (GVS) installation, including its full locked dependency tree and dependency links. The application does not need a `node_modules` directory. GVS itself retains its conventional `node_modules` layout.

Point the package's manifest entry at its real GVS directory and set `resolution` to `node`:

```json
{
  "root": "/path/to/store/links/@/tool/1.0.0/<graph-hash>/node_modules/tool",
  "resolution": "node"
}
```

The application's dependency map still points to this package's instance ID. Imports originating inside the opted-out package use Node's normal resolution and its physical dependency links. The entry does not need a loader dependency map. Explicit paths to CAS modules remain loadable through the hooks. Map materialized transitive package instances to their GVS roots too, so application imports of those instances use the same modules. Multiple opted-out IDs can share a GVS root when pnpm deduplicates them.

Use pnpm's normal installer to populate GVS; copying just the selected package's files does not create a complete opt-out. The installer owns graph hashes, package import, linking, build policy, and store registration. Retain the installation that references those entries so store pruning can track their usage. The loader itself does not install packages or register its manifest with store pruning.

This is an explicit compatibility fallback, not an install-time static analyzer. It can materialize a large dependency tree, but ordinary GVS entries can be reused across projects. Packages outside that tree stay in CAS. A tool that directly reads application dependencies outside its own tree may still need additional opt-outs or resolver integration.

## Supported behavior

The loader supports ESM and CommonJS, relative modules, dynamic imports, `createRequire`, JSON imports, package `exports` and `imports`, conditional exports, export patterns, package self references, aliases, and workspace dependency contexts. Resolution uses `enhanced-resolve` with Node-oriented settings.

Stored modules have virtual file URLs under `.pnpm-loader/` next to the manifest. That directory is never created. These URLs retain package identity and source filenames; the loader reads the corresponding bytes from CAS and verifies their hashes. The virtual namespace is reserved and cannot also be a workspace root.

Only declared dependencies are available through bare imports. There is no fallback to an application's existing `node_modules` tree for a dependency missing from the manifest. Built-in modules continue to use Node's loader. Absolute paths returned by resolvers remain loadable. Relative local application files use Node's normal resolution. Packages without a `package.json` receive an in-memory empty manifest to keep resolution within their package boundary.

## Compatibility limits

This loader does not emulate a general filesystem. `import.meta.url`, `__filename`, `__dirname`, and `require.resolve()` for stored modules identify virtual locations. Passing them to ordinary `fs` APIs will not read store assets. Packages that read adjacent assets, scan directories, write into their package directory, or depend on a physical filename need materialization or a separate filesystem integration.

Native addons, WebAssembly, TypeScript, extensionless source files, bundled `node_modules` directories, and package-internal symlinks are not supported by this initial runtime. Native addons may also require neighboring shared libraries, even if their build output is cached.

Use `.mjs` or `"type": "module"` for ESM. Stored `.js` files without a package type are treated as CommonJS; Node's syntax detection for ambiguous `.js` files is not implemented. The resolver is not a promise of complete Node resolution parity.

The loader does not run lifecycle scripts, select side-effects cache entries, create command shims, or generate manifests from lockfiles. A package with install scripts can run from stored build outputs if its effective files otherwise meet these compatibility requirements. Runtime and development dependencies have the same filesystem constraints.

The manifest is trusted configuration, as a lockfile and preload script are. Dependency restrictions and blob verification do not sandbox package code.

## Repository compatibility check

See [repository compatibility results](./store-loader-compatibility.md) for the tested revisions, workloads, passing results, and remaining failures across pnpm and eleven external projects.

From a checkout with dependencies installed and compiled, run:

```sh
node pnpm/esm-loader/scripts/test-repository.mjs
```

This manual compatibility check first runs the existing CLI argument parser test suite as a baseline. It then snapshots the installed dependency graph into a temporary CAS, copies the compiled workspaces without `node_modules`, bundles the loader so its own dependencies require no package directories, and attempts the parser suite, a pnpm CLI suite, and unbundled CLI startup. It verifies that the temporary tree contains no `node_modules` directories or symlinks outside the tree. The original checkout is not modified. The fixture and logs are retained at the printed path.

This tests runtime compatibility against the installed packages. It does not test fetching, automatic installation, or generation of a manifest from a lockfile. Missing optional/platform dependencies from the installed graph are recorded in `missing-dependencies.json`.

The repository patch for `unrs-resolver` enables Node resolution with `UNRS_RESOLVER_NODE_RESOLUTION=1`. The compatibility check sets this automatically. It delegates to `createRequire().resolve()` and forwards conditional exports through a temporary synchronous hook. This mode uses Node semantics; unrs-specific aliases, TypeScript configuration, and custom extension searches are not implemented. Normal runs retain the native resolver.

The patched run indexed 1,793 packages and 37,807 files. The parser baseline passed 51 tests. Without `node_modules`, Jest resolved its configuration and started the parser suite, then failed when its VM runtime read `source-map-support.js` directly through `fs`. Zero tests executed. The CLI suite reached registry setup and required the external `pnpr-prepare` test binary. The unbundled CLI's `with current --version` and `with current help` commands succeeded; `with current` prevents switching to the repository's pinned package manager. The unbundled development build reports version `0.0.0`.

These results do not establish compatibility with pnpm's test suite. The command exits unsuccessfully while any scenario fails. Running Jest requires additional filesystem integration and native-tooling support or materialization. See [the install-mode plan](https://github.com/pnpm/tasks/issues/64).

The resolver patch has a separate regression test that runs from CAS without a native binding or `node_modules`:

```sh
node --test pnpm/esm-loader/scripts/test-unrs-resolver.mjs
```

### Selective materialization experiment

To try materializing only packages that the parser suite reads directly, pass the retained fixture directory to:

```sh
node pnpm/esm-loader/scripts/test-selective-repository.mjs /path/to/retained-fixture
```

This diagnostic retries the parser suite after each missing store-backed filesystem read. It verifies and copies that package's files into `materialized/<package-instance-hash>`, then maps the package to a physical root in a separate manifest. It preserves the original CAS manifest and records each attempt and the selected packages in `selective-results.json`. It stops on success or on a failure that materialization cannot address. This is an experiment, not an automatic installer policy.

The parser suite passed all 51 tests after materializing 71 of the 1,585 stored package instances: 656 files, 3.4 MiB. The other 1,514 package instances remained in CAS, and the fixture contained no `node_modules` directory or external symlink. The 208 workspace projects already had physical sources and compiled outputs.

The selected packages include Jest internals and dependencies of the code under test, such as `@pnpm/nopt` and `didyoumean2`. Jest's VM reads those sources too. This demonstrates that selective materialization works for this suite, but does not establish that materializing Jest alone is sufficient or that all remaining CAS packages were exercised.

### External repository checks

The standalone diagnostic scripts also accept installed pnpm workspaces. These checks ran on Linux with Node.js 26.10.0. They snapshot workspace files and dependency blobs, then verify that the snapshot contains neither `node_modules` directories nor symlinks outside it. They do not add an install-time analyzer or change the repositories' source code.

| Repository revision | Workload | Normal installation | Store-backed result |
| --- | --- | --- | --- |
| [Vue 4ab865a](https://github.com/vuejs/core/tree/4ab865a848a1da3d10fb674f857e5fff13094644) | Reactivity suite | 445 passed, 4 skipped | Same result after materializing 3 tooling packages; all-CAS startup needs a native Rolldown binding |
| [Vite 1003321](https://github.com/vitejs/vite/tree/10033218d239c927cdc375970b5741cce408e81b) | Utility suite | 131 passed | No tests executed; after materializing 2 tooling packages, Vitest's runner cannot resolve `magic-string` |
| [Svelte 020242d](https://github.com/sveltejs/svelte/tree/020242d6bef059df9ae8c13dc8dbff4c9b31e0ff) | Store suite | 33 passed | No tests executed; after materializing 8 tooling packages, Vite's runner cannot resolve `esm-env` |
| Same Svelte revision | Compile 100 repository components for client and server | 200 compilations succeeded | All 200 outputs match, with no registry packages materialized; 297 modules from 15 packages loaded from CAS |

Vue's snapshot contains 509 registry package instances and 18 workspace projects. Its materialized subset is `vitest`, `vite`, and `@rolldown/binding-linux-x64-gnu`: 154 files, 22.5 MiB. Vite's snapshot contains 1,150 registry package instances and 285 workspace projects. Svelte's contains 410 registry package instances and 3 workspace projects. Workspace sources and built outputs remain physical in every scenario.

Both `magic-string` and `esm-env` resolve successfully through native Node resolution with the loader. The remaining failures are in the test runners' resolution paths. These are compatibility probes, not full-repository test passes. The Svelte compiler comparison exercises the compiler directly and does not replace its Vitest suite.

After installing the pinned dependencies in each checkout, run these from the pnpm repository:

```sh
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/vue run --project unit packages/reactivity/__tests__ --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/vite run packages/vite/src/node/__tests__/utils.spec.ts --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/svelte run packages/svelte/tests/store/test.ts --maxWorkers=2 --configLoader native --no-cache
```

Vite's checkout needs `pnpm --filter vite run build-bundle` first. Each command runs the normal baseline, prints the retained snapshot directory, and exits nonzero if the all-CAS run fails. `--configLoader native` avoids bundling the test configuration with a separate resolver. `--no-cache` prevents Vitest from creating `node_modules/.vite` for its result cache.

Use the corresponding snapshot directory for the follow-up experiments:

```sh
node pnpm/esm-loader/scripts/test-selective-repository.mjs /vue-snapshot @rolldown/binding-linux-x64-gnu vite vitest
node pnpm/esm-loader/scripts/test-selective-repository.mjs /vite-snapshot @rolldown/binding-linux-x64-gnu vitest
node pnpm/esm-loader/scripts/test-selective-repository.mjs /svelte-snapshot vite vitest esbuild @esbuild/linux-x64
node pnpm/esm-loader/scripts/test-svelte-compiler.mjs /svelte-snapshot
```

The selective experiment accepts explicit package names and can additionally materialize packages identified by missing-file or unsupported-native-addon errors. It does not infer dependencies through static analysis. Its loader preload is inherited through `NODE_OPTIONS` so worker processes can resolve CAS packages too. Reports preserve package counts, exit statuses, and individual attempt logs. The compiler check uses the original all-CAS manifest and compares hashes of generated JavaScript, CSS, source maps, and warnings.

### GVS opt-out experiment

Create a fresh fixture with `test-repository.mjs` or `test-ecosystem.mjs`, then select packages explicitly:

```sh
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /vue-snapshot vitest
```

The diagnostic reads the original installation's current lockfile and maps the selected package instances to their locked contexts. It reuses pnpm's lockfile pruning and serialization utilities to prepare a separate installation, then runs `pnpm install --frozen-lockfile --ignore-scripts` with the global virtual store enabled. It does not re-resolve versions or run project hooks; the snapshots already include hook results. The installed graph includes required dependencies and the optional dependencies applicable to the target platform.

The generated `.pnpm-gvs.json` points the installed closure at normal GVS entries with native resolution. `gvs-results.json` records the selection, physical roots, skipped optional dependencies, dependencies exposed by the source installation but absent from its locked graph, and the suite's exit status. The copied application tree is checked for `node_modules` before and after execution. GVS and the retained staging installation are outside that tree and do contain `node_modules`.

This repository-only diagnostic requires compiled pnpm workspace utilities and an installed pnpm v12 CLI. It currently rejects opt-out trees containing local file or workspace links. In particular, Vite's Vitest depends on the local Vite workspace, so that probe cannot use this diagnostic yet. Lifecycle scripts are disabled in the experiment; packages requiring uncached build outputs need a separately authorized normal build.

On the pinned repositories above, opting out `vitest` installed 136 package instances for Vue; all 445 reactivity tests passed with 4 existing skips. Svelte's opt-out installed 128 package instances, but its suite still stopped at the application dependency `esm-env`, outside Vitest's tree.

The pnpm parser suite passed all 51 tests with these explicit selections:

```sh
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /pnpm-snapshot jest @rushstack/worker-pool ts-jest-resolver @pnpm/nopt didyoumean2 is-windows p-limit bole split2 tempy empathic read-yaml-file
```

Those names select 18 installed instances and map their closures to 360 distinct GVS directories for 362 manifest instances. Jest alone still failed on direct filesystem reads of test setup dependencies. These are scoped compatibility checks, not full repository-suite passes.
