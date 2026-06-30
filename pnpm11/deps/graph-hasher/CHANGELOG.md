# @pnpm/calc-dep-state

## 1100.2.6

### Patch Changes

- Updated dependencies [bae694f]
- Updated dependencies [a84d2a1]
  - @pnpm/resolving.resolver-base@1100.5.0
  - @pnpm/lockfile.utils@1100.1.0
  - @pnpm/lockfile.types@1100.0.12

## 1100.2.5

### Patch Changes

- Updated dependencies [f20ad8f]
- Updated dependencies [681b593]
- Updated dependencies [a31faa7]
  - @pnpm/lockfile.utils@1100.0.13
  - @pnpm/types@1101.3.2
  - @pnpm/deps.path@1100.0.8
  - @pnpm/engine.runtime.system-version@1100.0.3
  - @pnpm/lockfile.types@1100.0.11
  - @pnpm/resolving.resolver-base@1100.4.2

## 1100.2.4

### Patch Changes

- bf1b731: Require trusted package identity before package-name `allowBuilds` entries can approve lifecycle scripts for git, git-hosted tarball, direct tarball, and local directory artifacts. To approve one of those artifacts explicitly, use its peer-suffix-free lockfile depPath as the `allowBuilds` key. Lockfile verification now rejects lockfiles where a registry-style dependency path (`name@semver`) is backed by a git, directory, or git-hosted tarball resolution (`ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`), so the dependency path is a reliable artifact identity by the time scripts can run.
- Updated dependencies [bf1b731]
  - @pnpm/types@1101.3.1
  - @pnpm/deps.path@1100.0.7
  - @pnpm/engine.runtime.system-version@1100.0.2
  - @pnpm/lockfile.types@1100.0.10
  - @pnpm/lockfile.utils@1100.0.12
  - @pnpm/resolving.resolver-base@1100.4.1

## 1100.2.3

### Patch Changes

- Updated dependencies [a017bf3]
- Updated dependencies [6d17b66]
  - @pnpm/types@1101.3.0
  - @pnpm/resolving.resolver-base@1100.4.0
  - @pnpm/deps.path@1100.0.6
  - @pnpm/engine.runtime.system-version@1100.0.1
  - @pnpm/lockfile.types@1100.0.9
  - @pnpm/lockfile.utils@1100.0.11

## 1100.2.2

### Patch Changes

- Updated dependencies [e55f4b5]
- Updated dependencies [35d2355]
  - @pnpm/lockfile.utils@1100.0.10
  - @pnpm/engine.runtime.system-version@1100.0.0
  - @pnpm/types@1101.2.0
  - @pnpm/deps.path@1100.0.5
  - @pnpm/lockfile.types@1100.0.8
  - @pnpm/resolving.resolver-base@1100.3.1

## 1100.2.1

### Patch Changes

- Updated dependencies [1627943]
- Updated dependencies [64afc92]
  - @pnpm/resolving.resolver-base@1100.3.0
  - @pnpm/types@1101.1.1
  - @pnpm/lockfile.types@1100.0.7
  - @pnpm/lockfile.utils@1100.0.9
  - @pnpm/deps.path@1100.0.4
  - @pnpm/engine.runtime.system-node-version@1100.1.1

## 1100.2.0

### Minor Changes

- 5dc8be8: **fix**: resolve the GVS hash's engine portion per-snapshot when a dependency declares its own `engines.runtime`, instead of using an install-wide value.

  Pnpm's resolver desugars a dep's `engines.runtime` into `dependencies.node: 'runtime:<version>'`, and the bin linker spawns that dep's lifecycle scripts through the pinned Node downloaded into `<pkgDir>/node_modules/node/`. The GVS hash and the side-effects-cache key prefix were still anchored to the install-wide runtime — so a pinning snapshot's slot encoded the wrong Node major, and a reinstall on the same host could read the cached side-effects under a key whose `<platform>;<arch>;node<major>` triple disagreed with the Node the build actually ran on.

  Per-snapshot resolution now matches what `bins/linker` already does on a per-package basis:

  - `@pnpm/deps.graph-hasher` adds `readSnapshotRuntimePin(children)` — reads the `node` entry from one snapshot's graph children and extracts the version from a `node@runtime:` value. Pairs with the existing `findRuntimeNodeVersion(snapshotKeys)` install-wide fallback (also now exported from `@pnpm/deps.graph-hasher` rather than `@pnpm/engine.runtime.system-node-version`, where it was a poor fit — `system-node-version` is about probing the host Node, not parsing lockfile-derived strings).
  - `calcDepState` and `calcGraphNodeHash` consult `readSnapshotRuntimePin(graph[depPath].children)` first and only fall back to the install-wide `nodeVersion` parameter when the snapshot doesn't pin its own Node.

  Pacquet mirrors the same precedence at the `calc_graph_node_hash` call site in `package-manager/src/virtual_store_layout.rs` — a new `find_own_runtime_node_major(snapshot)` helper reads each snapshot's `dependencies` for a `node` entry with `Prefix::Runtime` and overrides the install-wide engine when present.

  On upgrade, snapshots of dependencies that declare their own `engines.runtime` re-hash under that dep's pinned Node instead of the install-wide value. The old slots become prune-eligible. Closes [#11690](https://github.com/pnpm/pnpm/issues/11690).

### Patch Changes

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

- Updated dependencies [4195766]
- Updated dependencies [31538bf]
- Updated dependencies [3ddde2b]
  - @pnpm/resolving.resolver-base@1100.2.0
  - @pnpm/engine.runtime.system-node-version@1100.1.0
  - @pnpm/lockfile.types@1100.0.6
  - @pnpm/lockfile.utils@1100.0.8

## 1100.1.5

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/types@1101.1.0
  - @pnpm/deps.path@1100.0.3
  - @pnpm/lockfile.types@1100.0.5
  - @pnpm/lockfile.utils@1100.0.7
  - @pnpm/resolving.resolver-base@1100.1.3

## 1100.1.4

### Patch Changes

- Updated dependencies [cfa271b]
  - @pnpm/lockfile.utils@1100.0.6

## 1100.1.3

### Patch Changes

- Updated dependencies [27425d7]
  - @pnpm/lockfile.types@1100.0.4
  - @pnpm/lockfile.utils@1100.0.5
  - @pnpm/resolving.resolver-base@1100.1.2

## 1100.1.2

### Patch Changes

- 184ce26: Fix the package name in README.md.
- Updated dependencies [184ce26]
- Updated dependencies [6b891a5]
  - @pnpm/resolving.resolver-base@1100.1.1
  - @pnpm/deps.path@1100.0.2
  - @pnpm/lockfile.utils@1100.0.4
  - @pnpm/lockfile.types@1100.0.3

## 1100.1.1

### Patch Changes

- @pnpm/lockfile.utils@1100.0.3

## 1100.1.0

### Minor Changes

- 72c1e05: Fix: different platform variants of the same runtime (e.g. `node@runtime:25.9.0` glibc vs. musl) no longer share a single global-virtual-store entry. The virtual store path now incorporates the selected variant's integrity, so installs with different `--os`/`--cpu`/`--libc` end up in separate directories and `pnpm add --libc=musl node@runtime:<v>` reliably fetches the musl binary even when the glibc variant is already cached.

### Patch Changes

- Updated dependencies [72c1e05]
  - @pnpm/resolving.resolver-base@1100.1.0
  - @pnpm/lockfile.types@1100.0.2
  - @pnpm/lockfile.utils@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/deps.path@1100.0.1
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/lockfile.utils@1100.0.1

## 1003.0.0

### Major Changes

- 5f73b0f: Runtime dependencies are always linked from the global virtual store [#10233](https://github.com/pnpm/pnpm/pull/10233).
- 491a84f: This package is now pure ESM.
- c4045fc: **Semi-breaking.** Changed the location of unscoped packages in the virtual global store. They will now be stored under a directory named `@` to maintain a uniform 4-level directory depth.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- a8f016c: Store config dependency and package manager integrity info in `pnpm-lock.yaml` instead of inlining it in `pnpm-workspace.yaml`. The workspace manifest now contains only clean version specifiers for `configDependencies`, while the resolved versions, integrity hashes, and tarball URLs are recorded in the lockfile as a separate YAML document. The env lockfile section also stores `packageManagerDependencies` resolved during version switching and self-update. Projects using the old inline-hash format are automatically migrated on install.
- cd743ef: Use `allowBuilds` config to compute engine-agnostic GVS hashes for pure-JS packages [#10837](https://github.com/pnpm/pnpm/issues/10837).

  When the global virtual store is enabled, packages that are not allowed to build (and don't transitively depend on packages that are) now get hashes that don't include the engine name (platform, architecture, Node.js major version). This means ~95% of packages in the GVS survive Node.js upgrades and architecture changes without re-import.

- 38b8e35: Support for custom resolvers and fetchers.

### Patch Changes

- 1e6de25: Fix dependency graph hash calculation for runtime dependencies (like Node.js, Deno).
- Updated dependencies [5f73b0f]
- Updated dependencies [c55c614]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [606f53e]
- Updated dependencies [491a84f]
- Updated dependencies [075aa99]
- Updated dependencies [3bf5e21]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [394d88c]
- Updated dependencies [1e6de25]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/deps.path@1002.0.0
  - @pnpm/constants@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/lockfile.utils@1004.0.0
  - @pnpm/crypto.object-hasher@1001.0.0

## 1002.0.8

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/lockfile.types@1002.0.2
  - @pnpm/lockfile.utils@1003.0.3
  - @pnpm/dependency-path@1001.1.3

## 1002.0.7

### Patch Changes

- @pnpm/dependency-path@1001.1.2
- @pnpm/lockfile.utils@1003.0.2

## 1002.0.6

### Patch Changes

- Updated dependencies [6365bc4]
  - @pnpm/constants@1001.3.1

## 1002.0.5

### Patch Changes

- Updated dependencies [e792927]
  - @pnpm/types@1000.8.0
  - @pnpm/lockfile.types@1002.0.1
  - @pnpm/lockfile.utils@1003.0.1
  - @pnpm/dependency-path@1001.1.1

## 1002.0.4

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
- Updated dependencies [f91922c]
  - @pnpm/dependency-path@1001.1.0
  - @pnpm/constants@1001.3.0
  - @pnpm/lockfile.types@1002.0.0
  - @pnpm/lockfile.utils@1003.0.0

## 1002.0.3

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [2e85f29]
- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
  - @pnpm/types@1000.7.0
  - @pnpm/lockfile.utils@1002.1.0
  - @pnpm/lockfile.types@1001.1.0
  - @pnpm/constants@1001.2.0
  - @pnpm/dependency-path@1001.0.2

## 1002.0.2

### Patch Changes

- @pnpm/dependency-path@1001.0.1
- @pnpm/lockfile.utils@1002.0.1

## 1002.0.1

### Patch Changes

- Updated dependencies [540986f]
  - @pnpm/dependency-path@1001.0.0
  - @pnpm/lockfile.utils@1002.0.0

## 1002.0.0

### Major Changes

- b0ead51: Renamed `isBuilt` option to `includeDepGraphHash`.
- b3898db: **Semi-breaking.** The keys used for side-effects caches have changed. If you have a side-effects cache generated by a previous version of pnpm, the new version will not use it and will create a new cache instead [#9605](https://github.com/pnpm/pnpm/pull/9605).

### Minor Changes

- b0ead51: Added `iterateHashedGraphNodes`.

### Patch Changes

- Updated dependencies [b0ead51]
  - @pnpm/crypto.object-hasher@1000.1.0
  - @pnpm/lockfile.utils@1001.0.12

## 1001.0.13

### Patch Changes

- Updated dependencies [c00360b]
- Updated dependencies [5ec7255]
  - @pnpm/object.key-sorting@1000.0.1
  - @pnpm/types@1000.6.0
  - @pnpm/lockfile.types@1001.0.8
  - @pnpm/lockfile.utils@1001.0.11
  - @pnpm/dependency-path@1000.0.9

## 1001.0.12

### Patch Changes

- Updated dependencies [5b73df1]
  - @pnpm/types@1000.5.0
  - @pnpm/lockfile.utils@1001.0.10
  - @pnpm/lockfile.types@1001.0.7
  - @pnpm/dependency-path@1000.0.8

## 1001.0.11

### Patch Changes

- @pnpm/lockfile.utils@1001.0.9

## 1001.0.10

### Patch Changes

- Updated dependencies [750ae7d]
  - @pnpm/types@1000.4.0
  - @pnpm/lockfile.types@1001.0.6
  - @pnpm/lockfile.utils@1001.0.8
  - @pnpm/dependency-path@1000.0.7

## 1001.0.9

### Patch Changes

- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/types@1000.3.0
  - @pnpm/lockfile.types@1001.0.5
  - @pnpm/lockfile.utils@1001.0.7
  - @pnpm/dependency-path@1000.0.6

## 1001.0.8

### Patch Changes

- @pnpm/lockfile.utils@1001.0.6

## 1001.0.7

### Patch Changes

- @pnpm/dependency-path@1000.0.5
- @pnpm/lockfile.utils@1001.0.5

## 1001.0.6

### Patch Changes

- Updated dependencies [a5e4965]
  - @pnpm/types@1000.2.1
  - @pnpm/dependency-path@1000.0.4
  - @pnpm/lockfile.types@1001.0.4
  - @pnpm/lockfile.utils@1001.0.4

## 1001.0.5

### Patch Changes

- Updated dependencies [8fcc221]
  - @pnpm/types@1000.2.0
  - @pnpm/lockfile.types@1001.0.3
  - @pnpm/lockfile.utils@1001.0.3
  - @pnpm/dependency-path@1000.0.3

## 1001.0.4

### Patch Changes

- Updated dependencies [fee898f]
  - @pnpm/object.key-sorting@1000.0.0

## 1001.0.3

### Patch Changes

- Updated dependencies [3717340]
  - @pnpm/crypto.object-hasher@1000.0.1

## 1001.0.2

### Patch Changes

- Updated dependencies [9a44e6c]
- Updated dependencies [b562deb]
  - @pnpm/constants@1001.1.0
  - @pnpm/types@1000.1.1
  - @pnpm/lockfile.types@1001.0.2
  - @pnpm/lockfile.utils@1001.0.2
  - @pnpm/dependency-path@1000.0.2

## 1001.0.1

### Patch Changes

- Updated dependencies [9591a18]
  - @pnpm/types@1000.1.0
  - @pnpm/lockfile.types@1001.0.1
  - @pnpm/lockfile.utils@1001.0.1
  - @pnpm/dependency-path@1000.0.1

## 1001.0.0

### Major Changes

- a76da0c: Removed lockfile conversion from v6 to v9. If you need to convert lockfile v6 to v9, use pnpm CLI v9.

### Patch Changes

- Updated dependencies [d2e83b0]
- Updated dependencies [6483b64]
- Updated dependencies [a76da0c]
  - @pnpm/constants@1001.0.0
  - @pnpm/lockfile.types@1001.0.0
  - @pnpm/lockfile.utils@1001.0.0

## 7.0.11

### Patch Changes

- Updated dependencies [19d5b51]
- Updated dependencies [8108680]
- Updated dependencies [dcd2917]
- Updated dependencies [501c152]
- Updated dependencies [d55b259]
- Updated dependencies [c4f5231]
  - @pnpm/constants@10.0.0
  - @pnpm/dependency-path@6.0.0
  - @pnpm/crypto.object-hasher@3.0.0
  - @pnpm/lockfile.utils@1.0.5

## 7.0.10

### Patch Changes

- @pnpm/dependency-path@5.1.7
- @pnpm/lockfile.utils@1.0.4

## 7.0.9

### Patch Changes

- Updated dependencies [83681da]
  - @pnpm/constants@9.0.0

## 7.0.8

### Patch Changes

- Updated dependencies [d500d9f]
  - @pnpm/types@12.2.0
  - @pnpm/lockfile.types@1.0.3
  - @pnpm/lockfile.utils@1.0.3
  - @pnpm/dependency-path@5.1.6

## 7.0.7

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0
  - @pnpm/lockfile.types@1.0.2
  - @pnpm/lockfile.utils@1.0.2
  - @pnpm/dependency-path@5.1.5

## 7.0.6

### Patch Changes

- Updated dependencies [cb006df]
  - @pnpm/lockfile.types@1.0.1
  - @pnpm/types@12.0.0
  - @pnpm/lockfile.utils@1.0.1
  - @pnpm/dependency-path@5.1.4

## 7.0.5

### Patch Changes

- Updated dependencies [c5ef9b0]
- Updated dependencies [797ef0f]
  - @pnpm/lockfile.utils@1.0.0
  - @pnpm/lockfile.types@1.0.0

## 7.0.4

### Patch Changes

- Updated dependencies [0ef168b]
  - @pnpm/types@11.1.0
  - @pnpm/lockfile-types@7.1.3
  - @pnpm/lockfile-utils@11.0.4
  - @pnpm/dependency-path@5.1.3

## 7.0.3

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-utils@11.0.3
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/dependency-path@5.1.2

## 7.0.2

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/lockfile-utils@11.0.2
  - @pnpm/dependency-path@5.1.1

## 7.0.1

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/dependency-path@5.1.0
  - @pnpm/lockfile-types@7.1.0
  - @pnpm/lockfile-utils@11.0.1

## 7.0.0

### Major Changes

- Breaking changes to the API.

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/dependency-path@5.0.0

## 6.0.1

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [cdd8365]
- Updated dependencies [c692f80]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [d636eed]
- Updated dependencies [27a96a8]
- Updated dependencies [730929e]
- Updated dependencies [98a1266]
  - @pnpm/dependency-path@3.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/lockfile-types@6.0.0
  - @pnpm/crypto.object-hasher@2.0.0

## 5.0.0

### Major Changes

- 0c383327e: Reduce the length of the side-effects cache key. Instead of saving a stringified object composed from the dependency versions of the package, use the hash calculated from the said object [#7563](https://github.com/pnpm/pnpm/pull/7563).

### Patch Changes

- Updated dependencies [0c383327e]
  - @pnpm/crypto.object-hasher@1.0.0

## 4.1.5

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/dependency-path@2.1.7

## 4.1.4

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/dependency-path@2.1.6

## 4.1.3

### Patch Changes

- @pnpm/lockfile-types@5.1.3
- @pnpm/dependency-path@2.1.5

## 4.1.2

### Patch Changes

- @pnpm/lockfile-types@5.1.2
- @pnpm/dependency-path@2.1.4

## 4.1.1

### Patch Changes

- @pnpm/lockfile-types@5.1.1
- @pnpm/dependency-path@2.1.3

## 4.1.0

### Minor Changes

- 16bbac8d5: Add `lockfileToDepGraph` function.

## 4.0.2

### Patch Changes

- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1

## 4.0.1

### Patch Changes

- Updated dependencies [9c4ae87bd]
  - @pnpm/constants@7.1.0

## 4.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/constants@7.0.0

## 3.0.2

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0

## 3.0.1

### Patch Changes

- 285ff09ba: Calculate the cache key differently when scripts are ignored.

## 3.0.0

### Major Changes

- 2a34b21ce: Changed the order of arguments in calcDepState and added an optional last argument for patchFileHash.

## 2.0.1

### Patch Changes

- Updated dependencies [1267e4eff]
  - @pnpm/constants@6.1.0

## 2.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/constants@6.0.0

## 1.0.0

### Major Changes

- 1cadc231a: Initial release.
