# @pnpm/env.system-node-version

## 1100.1.0

### Minor Changes

- 3ddde2b: **fix**: anchor the side-effects-cache key and global-virtual-store hash to the project's script-runner Node — `engines.runtime` pin when present, shell `node` otherwise — instead of pnpm's own runtime.

  `ENGINE_NAME` (the `<platform>;<arch>;node<major>` prefix used as the side-effects-cache key and the engine portion of the GVS hash) was computed from `process.version` — the Node that runs pnpm itself. That was wrong in two situations:

  1. **`@pnpm/exe` SEA bundle.** The bundle has its own embedded Node, not the `node` on the user's `PATH` that actually spawns lifecycle scripts. Two pnpm installations on the same machine (one SEA, one npm-package) therefore disagreed on the cache key, partitioning the side-effects cache and the global virtual store across two Node majors even though both installs would run scripts on the same shell `node`.
  2. **`engines.runtime` / `devEngines.runtime` pin.** When a project pins a Node version via `devEngines.runtime` (pnpm v11+), pnpm downloads that Node into `node_modules/node/` and uses it to run lifecycle scripts. But the hash still anchored to whichever Node ran pnpm itself, not to the pinned Node — so two installs of the same project with two different runner Nodes would still disagree on the GVS slot path even though scripts run on the same pinned Node.

  Three changes:

  - `@pnpm/engine.runtime.system-node-version` now exports `engineName(nodeVersion?)`. Resolves the version in this order: explicit override → `getSystemNodeVersion()` (which already prefers `node --version` over `process.version` in SEA contexts) → `process.version`.
  - `@pnpm/deps.graph-hasher` now exports `findRuntimeNodeVersion(snapshotKeys)` — scans an iterable of lockfile snapshot keys for a `node@runtime:<version>` entry and returns its bare version string. `calcDepState` and `calcGraphNodeHash`/`iterateHashedGraphNodes` accept a `nodeVersion?` (in the options bag for the first, as a trailing parameter / ctx field for the others), forwarded to `engineName()`. The default (no override) preserves the pre-change behaviour. The legacy `ENGINE_NAME` constant in `@pnpm/constants` is unchanged so external consumers and existing tests keep working; in non-SEA, non-pinned contexts every value lines up.
  - Every install-side caller of the graph-hasher (`@pnpm/installing.deps-resolver`, `@pnpm/installing.deps-restorer`, `@pnpm/installing.deps-installer`, `@pnpm/building.during-install`, `@pnpm/building.after-install`, `@pnpm/deps.graph-builder`) now derives the project's pinned runtime via `findRuntimeNodeVersion(Object.keys(graph))` once per invocation and threads it through.

  On upgrade, two one-time GVS slot churns are possible:

  - **SEA-pnpm users** without a runtime pin: slots that previously hashed under the embedded-Node major (e.g. `node26`) now hash under the shell-Node major (e.g. `node24`), matching what pacquet, the npm-published `pnpm` package, and any other pnpm-compatible tool already produce.
  - **Projects with a `devEngines.runtime` pin**: slots that previously hashed under the runner's Node major now hash under the pinned Node major, matching what the lifecycle scripts will actually run on.

  In both cases the old slots become prune-eligible.

## 1100.0.3

### Patch Changes

- @pnpm/cli.meta@1100.0.3

## 1100.0.2

### Patch Changes

- 184ce26: Fix the package name in README.md.
- Updated dependencies [184ce26]
  - @pnpm/cli.meta@1100.0.2

## 1100.0.1

### Patch Changes

- @pnpm/cli.meta@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Patch Changes

- Updated dependencies [491a84f]
- Updated dependencies [7d2fd48]
  - @pnpm/cli.meta@1001.0.0

## 1000.0.11

### Patch Changes

- @pnpm/cli-meta@1000.0.11

## 1000.0.10

### Patch Changes

- @pnpm/cli-meta@1000.0.10

## 1000.0.9

### Patch Changes

- @pnpm/cli-meta@1000.0.9

## 1000.0.8

### Patch Changes

- @pnpm/cli-meta@1000.0.8

## 1000.0.7

### Patch Changes

- @pnpm/cli-meta@1000.0.7

## 1000.0.6

### Patch Changes

- @pnpm/cli-meta@1000.0.6

## 1000.0.5

### Patch Changes

- @pnpm/cli-meta@1000.0.5

## 1000.0.4

### Patch Changes

- @pnpm/cli-meta@1000.0.4

## 1000.0.3

### Patch Changes

- @pnpm/cli-meta@1000.0.3

## 1000.0.2

### Patch Changes

- @pnpm/cli-meta@1000.0.2

## 1000.0.1

### Patch Changes

- @pnpm/cli-meta@1000.0.1

## 1.0.1

### Patch Changes

- e476b07: Don't crash if the `use-node-version` setting is used and the system has no Node.js installed [#8769](https://github.com/pnpm/pnpm/issues/8769).

## 1.0.0

### Major Changes

- d04f7f2: Initial release.
