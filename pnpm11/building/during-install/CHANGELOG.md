# @pnpm/building.during-install

## 1102.0.2

### Patch Changes

- Updated dependencies [bae694f]
- Updated dependencies [852d537]
  - @pnpm/store.controller-types@1100.1.6
  - @pnpm/error@1100.0.1
  - @pnpm/deps.graph-hasher@1100.2.6
  - @pnpm/exec.lifecycle@1100.1.1
  - @pnpm/bins.linker@1100.0.16
  - @pnpm/config.reader@1101.10.1
  - @pnpm/patching.apply-patch@1100.0.3
  - @pnpm/pkg-manifest.reader@1100.0.9
  - @pnpm/worker@1100.2.2
  - @pnpm/fs.hard-link-dir@1100.0.2

## 1102.0.1

### Patch Changes

- Updated dependencies [302a2f7]
- Updated dependencies [3d1fd20]
- Updated dependencies [0474a9c]
  - @pnpm/config.reader@1101.10.0
  - @pnpm/bins.linker@1100.0.15
  - @pnpm/exec.lifecycle@1100.1.0
  - @pnpm/worker@1100.2.1

## 1102.0.0

### Patch Changes

- 61810aa: Added a new setting `frozenStore` (`--frozen-store`) that lets `pnpm install` run against a package store on a read-only filesystem (e.g. a Nix store, a read-only bind mount, an OCI layer). When enabled, pnpm opens the store's SQLite `index.db` through the `immutable=1` URI — bypassing the WAL/`-shm` sidecar creation that otherwise fails on a read-only directory — and suppresses every store-write path (the `index.db` writer and the project-registry write). Pair it with `--offline --frozen-lockfile` against a fully-populated store. Under the global virtual store, package directories live inside the store, so if the store is missing the build output of a package whose lifecycle scripts are approved (or that has a patch), pnpm fails up front with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` rather than crashing mid-build on a read-only write — seed the store with those builds first. Incompatible with `--force` and with a configured pnpr server, since both write into the store; the side-effects cache is likewise not written under `frozenStore`. If the store is missing its content directory, the install fails fast with `ERR_PNPM_FROZEN_STORE_INCOMPLETE` rather than attempting to initialize it. The read-only `immutable=1` open requires Node.js >=22.15.0, >=23.11.0, or >=24.0.0; on older runtimes `--frozen-store` fails with a clear `ERR_PNPM_FROZEN_STORE_UNSUPPORTED_NODE` error. Bin-linking also tolerates a read-only store: under the global virtual store a package's bin source lives inside the store, so the `chmod` that makes it executable would be refused — with `EPERM`/`EACCES`, or with `EROFS` on a genuinely read-only filesystem. That `chmod` is redundant when the seed already ships its bins executable with a normalized shebang, so it is now skipped in that case, while a non-executable bin (or one still carrying a Windows CRLF shebang) on a read-only store still errors.
- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [61810aa]
- Updated dependencies [23716ed]
- Updated dependencies [681b593]
- Updated dependencies [a31faa7]
- Updated dependencies [cd8348c]
  - @pnpm/config.reader@1101.9.0
  - @pnpm/bins.linker@1100.0.14
  - @pnpm/worker@1100.2.0
  - @pnpm/exec.lifecycle@1100.0.18
  - @pnpm/types@1101.3.2
  - @pnpm/core-loggers@1100.2.1
  - @pnpm/deps.path@1100.0.8
  - @pnpm/fs.hard-link-dir@1100.0.2
  - @pnpm/patching.apply-patch@1100.0.2
  - @pnpm/deps.graph-hasher@1100.2.5
  - @pnpm/pkg-manifest.reader@1100.0.8
  - @pnpm/store.controller-types@1100.1.5

## 1101.0.18

### Patch Changes

- Updated dependencies [bc9ed78]
- Updated dependencies [f11b4fc]
- Updated dependencies [615c669]
  - @pnpm/config.reader@1101.8.0
  - @pnpm/core-loggers@1100.2.0
  - @pnpm/exec.lifecycle@1100.0.17
  - @pnpm/worker@1100.1.11
  - @pnpm/bins.linker@1100.0.13
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.1

## 1101.0.17

### Patch Changes

- bf1b731: Require trusted package identity before package-name `allowBuilds` entries can approve lifecycle scripts for git, git-hosted tarball, direct tarball, and local directory artifacts. To approve one of those artifacts explicitly, use its peer-suffix-free lockfile depPath as the `allowBuilds` key. Lockfile verification now rejects lockfiles where a registry-style dependency path (`name@semver`) is backed by a git, directory, or git-hosted tarball resolution (`ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`), so the dependency path is a reliable artifact identity by the time scripts can run.
- Updated dependencies [822beb5]
- Updated dependencies [3537020]
- Updated dependencies [894ea6a]
- Updated dependencies [6b5d91a]
- Updated dependencies [027196b]
- Updated dependencies [089484a]
- Updated dependencies [1017c36]
- Updated dependencies [bf1b731]
  - @pnpm/config.reader@1101.7.0
  - @pnpm/worker@1100.1.10
  - @pnpm/deps.graph-hasher@1100.2.4
  - @pnpm/types@1101.3.1
  - @pnpm/bins.linker@1100.0.12
  - @pnpm/core-loggers@1100.1.4
  - @pnpm/deps.path@1100.0.7
  - @pnpm/exec.lifecycle@1100.0.16
  - @pnpm/pkg-manifest.reader@1100.0.7
  - @pnpm/store.controller-types@1100.1.4
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.1

## 1101.0.16

### Patch Changes

- Updated dependencies [3b76b8e]
- Updated dependencies [a017bf3]
  - @pnpm/worker@1100.1.9
  - @pnpm/config.reader@1101.6.0
  - @pnpm/types@1101.3.0
  - @pnpm/bins.linker@1100.0.11
  - @pnpm/core-loggers@1100.1.3
  - @pnpm/deps.graph-hasher@1100.2.3
  - @pnpm/deps.path@1100.0.6
  - @pnpm/exec.lifecycle@1100.0.15
  - @pnpm/pkg-manifest.reader@1100.0.6
  - @pnpm/store.controller-types@1100.1.3
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.1

## 1101.0.15

### Patch Changes

- Updated dependencies [a39a83d]
  - @pnpm/config.reader@1101.5.0
  - @pnpm/exec.lifecycle@1100.0.14
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.1

## 1101.0.14

### Patch Changes

- Updated dependencies [a23956e]
- Updated dependencies [aa6149d]
- Updated dependencies [26a7d63]
- Updated dependencies [35d2355]
  - @pnpm/config.reader@1101.4.1
  - @pnpm/worker@1100.1.8
  - @pnpm/patching.apply-patch@1100.0.1
  - @pnpm/types@1101.2.0
  - @pnpm/bins.linker@1100.0.10
  - @pnpm/deps.graph-hasher@1100.2.2
  - @pnpm/core-loggers@1100.1.2
  - @pnpm/deps.path@1100.0.5
  - @pnpm/exec.lifecycle@1100.0.14
  - @pnpm/pkg-manifest.reader@1100.0.5
  - @pnpm/store.controller-types@1100.1.2
  - @pnpm/fs.hard-link-dir@1100.0.1

## 1101.0.13

### Patch Changes

- Updated dependencies [3b62f9d]
- Updated dependencies [212315d]
  - @pnpm/config.reader@1101.4.0
  - @pnpm/bins.linker@1100.0.9
  - @pnpm/exec.lifecycle@1100.0.13

## 1101.0.12

### Patch Changes

- Updated dependencies [3687b0e]
- Updated dependencies [ced20cb]
- Updated dependencies [9cb48bb]
- Updated dependencies [d1b340f]
- Updated dependencies [64afc92]
  - @pnpm/config.reader@1101.3.3
  - @pnpm/exec.lifecycle@1100.0.12
  - @pnpm/types@1101.1.1
  - @pnpm/deps.graph-hasher@1100.2.1
  - @pnpm/store.controller-types@1100.1.1
  - @pnpm/bins.linker@1100.0.8
  - @pnpm/core-loggers@1100.1.1
  - @pnpm/deps.path@1100.0.4
  - @pnpm/pkg-manifest.reader@1100.0.4
  - @pnpm/worker@1100.1.7
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.0

## 1101.0.11

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
- Updated dependencies [020ac45]
- Updated dependencies [d3f8408]
- Updated dependencies [3ddde2b]
- Updated dependencies [5dc8be8]
- Updated dependencies [4a79336]
- Updated dependencies [a62f959]
- Updated dependencies [ba2c884]
- Updated dependencies [8df408c]
  - @pnpm/store.controller-types@1100.1.0
  - @pnpm/config.reader@1101.3.2
  - @pnpm/deps.graph-hasher@1100.2.0
  - @pnpm/core-loggers@1100.1.0
  - @pnpm/exec.lifecycle@1100.0.11
  - @pnpm/worker@1100.1.6
  - @pnpm/bins.linker@1100.0.7
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.0

## 1101.0.10

### Patch Changes

- Updated dependencies [c2c2890]
  - @pnpm/store.controller-types@1100.0.7
  - @pnpm/bins.linker@1100.0.6
  - @pnpm/config.reader@1101.3.1
  - @pnpm/exec.lifecycle@1100.0.10
  - @pnpm/worker@1100.1.5
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.0

## 1101.0.9

### Patch Changes

- Updated dependencies [b4f8f47]
  - @pnpm/bins.linker@1100.0.5
  - @pnpm/exec.lifecycle@1100.0.9

## 1101.0.8

### Patch Changes

- Updated dependencies [b61e268]
- Updated dependencies [e1e29c1]
  - @pnpm/config.reader@1101.3.0
  - @pnpm/types@1101.1.0
  - @pnpm/bins.linker@1100.0.4
  - @pnpm/core-loggers@1100.0.2
  - @pnpm/deps.graph-hasher@1100.1.5
  - @pnpm/deps.path@1100.0.3
  - @pnpm/exec.lifecycle@1100.0.8
  - @pnpm/pkg-manifest.reader@1100.0.3
  - @pnpm/store.controller-types@1100.0.6
  - @pnpm/worker@1100.1.4
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.0

## 1101.0.7

### Patch Changes

- Updated dependencies [e9e876c]
  - @pnpm/config.reader@1101.2.2
  - @pnpm/worker@1100.1.3
  - @pnpm/exec.lifecycle@1100.0.7
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.0

## 1101.0.6

### Patch Changes

- @pnpm/deps.graph-hasher@1100.1.4

## 1101.0.5

### Patch Changes

- Updated dependencies [707a879]
  - @pnpm/config.reader@1101.2.1
  - @pnpm/deps.graph-hasher@1100.1.3
  - @pnpm/store.controller-types@1100.0.5
  - @pnpm/exec.lifecycle@1100.0.6
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.0
  - @pnpm/worker@1100.1.2

## 1101.0.4

### Patch Changes

- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0

## 1101.0.3

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4

## 1101.0.2

### Patch Changes

- 184ce26: Fix the package name in README.md.
- Updated dependencies [184ce26]
  - @pnpm/store.controller-types@1100.0.4
  - @pnpm/pkg-manifest.reader@1100.0.2
  - @pnpm/deps.graph-hasher@1100.1.2
  - @pnpm/exec.lifecycle@1100.0.5
  - @pnpm/config.reader@1101.1.3
  - @pnpm/bins.linker@1100.0.3
  - @pnpm/deps.path@1100.0.2
  - @pnpm/worker@1100.1.1
  - @pnpm/fs.hard-link-dir@1100.0.1
  - @pnpm/patching.apply-patch@1100.0.0

## 1101.0.1

### Patch Changes

- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2

## 1101.0.0

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/worker@1100.1.0
  - @pnpm/store.controller-types@1100.0.3
  - @pnpm/exec.lifecycle@1100.0.4
  - @pnpm/deps.graph-hasher@1100.1.1
  - @pnpm/config.reader@1101.1.1
  - @pnpm/fs.hard-link-dir@1100.0.0
  - @pnpm/patching.apply-patch@1100.0.0

## 1100.0.3

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/deps.graph-hasher@1100.1.0
  - @pnpm/exec.lifecycle@1100.0.3
  - @pnpm/store.controller-types@1100.0.2
  - @pnpm/worker@1100.0.2
  - @pnpm/fs.hard-link-dir@1100.0.0
  - @pnpm/patching.apply-patch@1100.0.0

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/config.reader@1101.0.0
  - @pnpm/bins.linker@1100.0.2
  - @pnpm/exec.lifecycle@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/bins.linker@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/core-loggers@1100.0.1
  - @pnpm/deps.graph-hasher@1100.0.1
  - @pnpm/deps.path@1100.0.1
  - @pnpm/exec.lifecycle@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.1
  - @pnpm/store.controller-types@1100.0.1
  - @pnpm/worker@1100.0.1
  - @pnpm/fs.hard-link-dir@1100.0.0
  - @pnpm/patching.apply-patch@1100.0.0

## 1000.0.0

### Major Changes

- 2fccb03: Initial release
- 05fb1ae: `ignoreBuilds` is now a set of DepPath.
- efb48dc: Replaced fetchingBundledManifest with fetching.
- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- 56a59df: Store the bundled manifest (name, version, bin, engines, scripts, etc.) directly in the package index file, eliminating the need to read `package.json` from the content-addressable store during resolution and installation. This reduces I/O and speeds up repeat installs [#10473](https://github.com/pnpm/pnpm/pull/10473).
- 7b1c189: Removed the deprecated `allowNonAppliedPatches` completely in favor of `allowUnusedPatches`.
  Remove `ignorePatchFailures` so all patch application failures should throw an error.

### Patch Changes

- 9b801c8: Fixed `strictDepBuilds` and `allowBuilds` checks being bypassed when a package's build side-effects are cached in the store. Packages with cached builds were skipped by `buildModules` (`isBuilt: true`) and never reached the `allowBuild` check. Now checks `allowBuild` for all packages with `requiresBuild` regardless of `isBuilt` state. Also detects packages whose build approval was revoked between installs.
- 56a59df: Defer patch errors until all patches in a group are applied, so that one failed patch does not prevent other patches from being attempted.
- 4362c06: `pnpm install` should build any dependencies that were added to `onlyBuiltDependencies` and were not built yet [#10256](https://github.com/pnpm/pnpm/pull/10256).
- Updated dependencies [7730a7f]
- Updated dependencies [5f73b0f]
- Updated dependencies [449dacf]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [7cec347]
- Updated dependencies [2a50b89]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [cd743ef]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [491a84f]
- Updated dependencies [62f760e]
- Updated dependencies [f0ae1b9]
- Updated dependencies [6e9cad3]
- Updated dependencies [50fbeca]
- Updated dependencies [cb228c9]
- Updated dependencies [0fd53e1]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [c4045fc]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [ba065f6]
- Updated dependencies [3bf5e21]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [a0e3a21]
- Updated dependencies [ee9fe58]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [780af09]
- Updated dependencies [96704a1]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [f40177f]
- Updated dependencies [71de2b3]
- Updated dependencies [4893853]
- Updated dependencies [10bc391]
- Updated dependencies [b09722f]
- Updated dependencies [38b8e35]
- Updated dependencies [b7f0f21]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [366cabe]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [9d3f00b]
- Updated dependencies [98a0410]
- Updated dependencies [efb48dc]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
- Updated dependencies [f871365]
  - @pnpm/config.reader@1005.0.0
  - @pnpm/deps.path@1002.0.0
  - @pnpm/deps.graph-hasher@1003.0.0
  - @pnpm/bins.linker@1001.0.0
  - @pnpm/store.controller-types@1005.0.0
  - @pnpm/worker@1001.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/fs.hard-link-dir@1001.0.0
  - @pnpm/pkg-manifest.reader@1001.0.0
  - @pnpm/core-loggers@1002.0.0
  - @pnpm/deps.graph-sequencer@1001.0.0
  - @pnpm/patching.apply-patch@1001.0.0
  - @pnpm/exec.lifecycle@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/patching.types@1001.0.0
