# @pnpm/config.deps-installer

## 1102.0.2

### Patch Changes

- a84d2a1: Add `@pnpm/resolving.tarball-url`, which builds and recognizes the canonical npm tarball URL of a package. It vendors `getNpmTarballUrl` (previously the external `get-npm-tarball-url` package) and adds `isCanonicalRegistryTarballUrl`, the predicate the lockfile writer uses to decide whether a tarball URL is derivable from name+version+registry (and can therefore be omitted from `pnpm-lock.yaml`).

  Exposing `isCanonicalRegistryTarballUrl` lets a custom resolver (pnpmfile `resolvers`) fronting a proxy that serves tarballs on a non-canonical path (e.g. an ephemeral `localhost:<port>`) rewrite the resolved tarball to the canonical form, so nothing host-specific is persisted to the lockfile. Previously this logic was private to `@pnpm/lockfile.utils`.

  Two correctness fixes are included while consolidating the logic: the scoped-package unescape now handles uppercase `%2F` as well as `%2f` (percent-encoding is case-insensitive), and protocol-insensitive comparison strips only a leading `http(s)://` scheme instead of splitting on the first `://` (which could truncate URLs containing a later `://`).

- Updated dependencies [bae694f]
- Updated dependencies [1cbb5f2]
- Updated dependencies [05b95ab]
- Updated dependencies [322f88f]
- Updated dependencies [fa7004b]
- Updated dependencies [a84d2a1]
- Updated dependencies [852d537]
  - @pnpm/resolving.npm-resolver@1102.1.0
  - @pnpm/installing.deps-resolver@1100.2.5
  - @pnpm/store.controller-types@1100.1.6
  - @pnpm/lockfile.utils@1100.1.0
  - @pnpm/network.fetch@1100.1.4
  - @pnpm/resolving.tarball-url@1100.0.0
  - @pnpm/error@1100.0.1
  - @pnpm/config.writer@1100.0.14
  - @pnpm/deps.graph-hasher@1100.2.6
  - @pnpm/lockfile.types@1100.0.12
  - @pnpm/store.controller@1102.0.2
  - @pnpm/lockfile.fs@1100.1.7
  - @pnpm/config.package-is-installable@1100.0.12
  - @pnpm/network.auth-header@1101.1.3
  - @pnpm/pkg-manifest.reader@1100.0.9
  - @pnpm/worker@1100.2.2
  - @pnpm/lockfile.pruner@1100.0.12

## 1102.0.1

### Patch Changes

- bee4bf4: Security: validate config dependency names and versions from the env lockfile (`pnpm-lock.yaml`) before using them to build filesystem paths. A committed lockfile with a traversal-shaped `configDependencies` name (such as `../../PWNED`) or version (such as `../../../PWNED`) could previously cause `pnpm install` to create symlinks or write package files outside `node_modules/.pnpm-config` and the store. Names must now be valid npm package names and versions must be exact semver versions; the same validation is applied to optional subdependencies of config dependencies, and to the legacy workspace-manifest format before any lockfile is written. See [GHSA-qrv3-253h-g69c](https://github.com/pnpm/pnpm/security/advisories/GHSA-qrv3-253h-g69c).
- Updated dependencies [29ab905]
- Updated dependencies [96bdd57]
- Updated dependencies [61969fb]
- Updated dependencies [5c12968]
- Updated dependencies [531f2a3]
- Updated dependencies [fe66535]
- Updated dependencies [817f99d]
  - @pnpm/resolving.npm-resolver@1102.0.1
  - @pnpm/installing.deps-resolver@1100.2.4
  - @pnpm/lockfile.fs@1100.1.6
  - @pnpm/store.controller@1102.0.1
  - @pnpm/worker@1100.2.1

## 1102.0.0

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [f648e9b]
- Updated dependencies [9b35a60]
- Updated dependencies [61810aa]
- Updated dependencies [f20ad8f]
- Updated dependencies [3a27141]
- Updated dependencies [681b593]
- Updated dependencies [d50d691]
- Updated dependencies [1310ab5]
- Updated dependencies [a31faa7]
  - @pnpm/installing.deps-resolver@1100.2.3
  - @pnpm/store.controller@1102.0.0
  - @pnpm/resolving.npm-resolver@1102.0.0
  - @pnpm/worker@1100.2.0
  - @pnpm/lockfile.utils@1100.0.13
  - @pnpm/network.auth-header@1101.1.2
  - @pnpm/types@1101.3.2
  - @pnpm/lockfile.fs@1100.1.5
  - @pnpm/config.package-is-installable@1100.0.11
  - @pnpm/core-loggers@1100.2.1
  - @pnpm/network.fetch@1100.1.3
  - @pnpm/deps.graph-hasher@1100.2.5
  - @pnpm/config.pick-registry-for-package@1100.0.9
  - @pnpm/config.writer@1100.0.13
  - @pnpm/lockfile.pruner@1100.0.11
  - @pnpm/lockfile.types@1100.0.11
  - @pnpm/pkg-manifest.reader@1100.0.8
  - @pnpm/store.controller-types@1100.1.5

## 1101.1.8

### Patch Changes

- Updated dependencies [f11b4fc]
- Updated dependencies [52be454]
  - @pnpm/core-loggers@1100.2.0
  - @pnpm/config.package-is-installable@1100.0.10
  - @pnpm/installing.deps-resolver@1100.2.2
  - @pnpm/network.fetch@1100.1.2
  - @pnpm/resolving.npm-resolver@1101.5.2
  - @pnpm/store.controller@1101.0.13
  - @pnpm/worker@1100.1.11

## 1101.1.7

### Patch Changes

- Updated dependencies [089484a]
- Updated dependencies [29a496a]
- Updated dependencies [bf1b731]
  - @pnpm/worker@1100.1.10
  - @pnpm/installing.deps-resolver@1100.2.1
  - @pnpm/deps.graph-hasher@1100.2.4
  - @pnpm/types@1101.3.1
  - @pnpm/config.package-is-installable@1100.0.9
  - @pnpm/config.pick-registry-for-package@1100.0.8
  - @pnpm/config.writer@1100.0.12
  - @pnpm/core-loggers@1100.1.4
  - @pnpm/lockfile.fs@1100.1.4
  - @pnpm/lockfile.pruner@1100.0.10
  - @pnpm/lockfile.types@1100.0.10
  - @pnpm/lockfile.utils@1100.0.12
  - @pnpm/network.auth-header@1101.1.1
  - @pnpm/network.fetch@1100.1.1
  - @pnpm/pkg-manifest.reader@1100.0.7
  - @pnpm/resolving.npm-resolver@1101.5.1
  - @pnpm/store.controller@1101.0.12
  - @pnpm/store.controller-types@1100.1.4

## 1101.1.6

### Patch Changes

- Updated dependencies [60a1eec]
- Updated dependencies [5192edf]
- Updated dependencies [3b76b8e]
- Updated dependencies [1c73e83]
- Updated dependencies [a017bf3]
- Updated dependencies [722b9cd]
- Updated dependencies [6d17b66]
  - @pnpm/network.fetch@1100.1.0
  - @pnpm/network.auth-header@1101.1.0
  - @pnpm/worker@1100.1.9
  - @pnpm/installing.deps-resolver@1100.2.0
  - @pnpm/types@1101.3.0
  - @pnpm/resolving.npm-resolver@1101.5.0
  - @pnpm/config.package-is-installable@1100.0.8
  - @pnpm/config.pick-registry-for-package@1100.0.7
  - @pnpm/config.writer@1100.0.11
  - @pnpm/core-loggers@1100.1.3
  - @pnpm/deps.graph-hasher@1100.2.3
  - @pnpm/lockfile.fs@1100.1.3
  - @pnpm/lockfile.pruner@1100.0.9
  - @pnpm/lockfile.types@1100.0.9
  - @pnpm/lockfile.utils@1100.0.11
  - @pnpm/pkg-manifest.reader@1100.0.6
  - @pnpm/store.controller@1101.0.11
  - @pnpm/store.controller-types@1100.1.3

## 1101.1.5

### Patch Changes

- Updated dependencies [6f382f4]
- Updated dependencies [122ab0a]
- Updated dependencies [1db05c6]
  - @pnpm/installing.deps-resolver@1100.1.6

## 1101.1.4

### Patch Changes

- Updated dependencies [39101f5]
- Updated dependencies [b1fa2d5]
- Updated dependencies [6235428]
- Updated dependencies [1e9ab29]
  - @pnpm/installing.deps-resolver@1100.1.5
  - @pnpm/network.fetch@1100.0.8
  - @pnpm/resolving.npm-resolver@1101.4.0
  - @pnpm/store.controller@1101.0.10

## 1101.1.3

### Patch Changes

- Updated dependencies [a23956e]
- Updated dependencies [aa6149d]
- Updated dependencies [ad84fff]
- Updated dependencies [e55f4b5]
- Updated dependencies [35d2355]
- Updated dependencies [0721d64]
  - @pnpm/network.auth-header@1101.0.0
  - @pnpm/worker@1100.1.8
  - @pnpm/installing.deps-resolver@1100.1.4
  - @pnpm/lockfile.utils@1100.0.10
  - @pnpm/types@1101.2.0
  - @pnpm/resolving.npm-resolver@1101.3.3
  - @pnpm/deps.graph-hasher@1100.2.2
  - @pnpm/lockfile.fs@1100.1.2
  - @pnpm/config.package-is-installable@1100.0.7
  - @pnpm/config.pick-registry-for-package@1100.0.6
  - @pnpm/config.writer@1100.0.10
  - @pnpm/core-loggers@1100.1.2
  - @pnpm/lockfile.pruner@1100.0.8
  - @pnpm/lockfile.types@1100.0.8
  - @pnpm/network.fetch@1100.0.7
  - @pnpm/pkg-manifest.reader@1100.0.5
  - @pnpm/store.controller@1101.0.9
  - @pnpm/store.controller-types@1100.1.2

## 1101.1.2

### Patch Changes

- 155af87: Fixed `pnpm add --config` leaving orphan entries in `pnpm-lock.env.yaml` (the optional subdependencies of the previously resolved version of the updated config dependency).
- Updated dependencies [3422cec]
- Updated dependencies [e0bd879]
- Updated dependencies [212315d]
  - @pnpm/installing.deps-resolver@1100.1.3
  - @pnpm/resolving.npm-resolver@1101.3.2
  - @pnpm/store.controller@1101.0.8

## 1101.1.1

### Patch Changes

- 2061c55: Mark optional subdependency snapshots of config dependencies with `optional: true` in the env lockfile, matching how optional dependencies are recorded elsewhere in `pnpm-lock.yaml`. Previously, snapshots for the platform-specific subdeps pulled in via a config dep's `optionalDependencies` were written as empty objects, which was inconsistent with the rest of the lockfile and made it look like those non-host platform variants were required.
- e5e7b72: Don't print "Installing config dependencies..." when config dependencies are already installed and nothing needs to be fetched, re-linked, or removed.
- Updated dependencies [097983f]
  - @pnpm/config.pick-registry-for-package@1100.0.5
  - @pnpm/resolving.npm-resolver@1101.3.1
  - @pnpm/installing.deps-resolver@1100.1.2
  - @pnpm/store.controller@1101.0.8

## 1101.1.0

### Minor Changes

- c8d8fde: `configDependencies` now resolve and install one level of `optionalDependencies` declared by the config dependency, with `os`/`cpu`/`libc` platform filtering applied at install time. This unlocks the esbuild/swc-style pattern where a package ships platform-specific binaries via `optionalDependencies` — a config dependency can now do the same and have the matching binary symlinked next to it in the global virtual store, so `require('pkg-platform-arch')` from inside the config dependency resolves correctly.

  The env lockfile records all platform variants regardless of host platform, so it remains portable across machines. Each entry in a config dependency's `optionalDependencies` must declare an exact version — ranges and tags are rejected to keep installs reproducible.

### Patch Changes

- Updated dependencies [9cb48bb]
- Updated dependencies [3a54205]
- Updated dependencies [1627943]
- Updated dependencies [64afc92]
  - @pnpm/lockfile.fs@1100.1.1
  - @pnpm/resolving.npm-resolver@1101.3.0
  - @pnpm/types@1101.1.1
  - @pnpm/installing.deps-resolver@1100.1.1
  - @pnpm/store.controller@1101.0.8
  - @pnpm/deps.graph-hasher@1100.2.1
  - @pnpm/lockfile.types@1100.0.7
  - @pnpm/lockfile.utils@1100.0.9
  - @pnpm/store.controller-types@1100.1.1
  - @pnpm/config.package-is-installable@1100.0.6
  - @pnpm/config.pick-registry-for-package@1100.0.4
  - @pnpm/config.writer@1100.0.9
  - @pnpm/core-loggers@1100.1.1
  - @pnpm/lockfile.pruner@1100.0.7
  - @pnpm/network.auth-header@1100.0.3
  - @pnpm/network.fetch@1100.0.6
  - @pnpm/pkg-manifest.reader@1100.0.4
  - @pnpm/worker@1100.1.7

## 1101.0.10

### Patch Changes

- Updated dependencies [963861c]
- Updated dependencies [4195766]
- Updated dependencies [31538bf]
- Updated dependencies [b6e2c8c]
- Updated dependencies [6e93f35]
- Updated dependencies [3ddde2b]
- Updated dependencies [5dc8be8]
- Updated dependencies [4a79336]
- Updated dependencies [2a9bd89]
  - @pnpm/resolving.npm-resolver@1101.2.0
  - @pnpm/store.controller-types@1100.1.0
  - @pnpm/installing.deps-resolver@1100.1.0
  - @pnpm/lockfile.fs@1100.1.0
  - @pnpm/deps.graph-hasher@1100.2.0
  - @pnpm/core-loggers@1100.1.0
  - @pnpm/lockfile.types@1100.0.6
  - @pnpm/lockfile.utils@1100.0.8
  - @pnpm/store.controller@1101.0.7
  - @pnpm/network.fetch@1100.0.5
  - @pnpm/lockfile.pruner@1100.0.6
  - @pnpm/worker@1100.1.6
  - @pnpm/config.writer@1100.0.8

## 1101.0.9

### Patch Changes

- Updated dependencies [50b33c1]
- Updated dependencies [18a464f]
- Updated dependencies [e526f89]
- Updated dependencies [180aee9]
- Updated dependencies [c2c2890]
  - @pnpm/resolving.npm-resolver@1101.1.1
  - @pnpm/network.fetch@1100.0.4
  - @pnpm/lockfile.fs@1100.0.8
  - @pnpm/installing.deps-resolver@1100.0.10
  - @pnpm/store.controller-types@1100.0.7
  - @pnpm/store.controller@1101.0.6
  - @pnpm/worker@1100.1.5

## 1101.0.8

### Patch Changes

- Updated dependencies [20e7aff]
- Updated dependencies [b61e268]
  - @pnpm/network.fetch@1100.0.3
  - @pnpm/resolving.npm-resolver@1101.1.0
  - @pnpm/types@1101.1.0
  - @pnpm/installing.deps-resolver@1100.0.9
  - @pnpm/config.pick-registry-for-package@1100.0.3
  - @pnpm/config.writer@1100.0.7
  - @pnpm/core-loggers@1100.0.2
  - @pnpm/deps.graph-hasher@1100.1.5
  - @pnpm/lockfile.fs@1100.0.7
  - @pnpm/lockfile.pruner@1100.0.5
  - @pnpm/lockfile.types@1100.0.5
  - @pnpm/lockfile.utils@1100.0.7
  - @pnpm/network.auth-header@1100.0.2
  - @pnpm/pkg-manifest.reader@1100.0.3
  - @pnpm/store.controller@1101.0.5
  - @pnpm/store.controller-types@1100.0.6
  - @pnpm/worker@1100.1.4

## 1101.0.7

### Patch Changes

- Updated dependencies [15e9e35]
  - @pnpm/resolving.npm-resolver@1101.0.3
  - @pnpm/store.controller@1101.0.4
  - @pnpm/worker@1100.1.3
  - @pnpm/installing.deps-resolver@1100.0.8

## 1101.0.6

### Patch Changes

- Updated dependencies [cfa271b]
  - @pnpm/lockfile.utils@1100.0.6
  - @pnpm/deps.graph-hasher@1100.1.4
  - @pnpm/installing.deps-resolver@1100.0.7
  - @pnpm/lockfile.fs@1100.0.6
  - @pnpm/store.controller@1101.0.3

## 1101.0.5

### Patch Changes

- Updated dependencies [27425d7]
  - @pnpm/lockfile.fs@1100.0.5
  - @pnpm/lockfile.types@1100.0.4
  - @pnpm/lockfile.utils@1100.0.5
  - @pnpm/installing.deps-resolver@1100.0.6
  - @pnpm/store.controller@1101.0.3
  - @pnpm/deps.graph-hasher@1100.1.3
  - @pnpm/lockfile.pruner@1100.0.4
  - @pnpm/resolving.npm-resolver@1101.0.2
  - @pnpm/store.controller-types@1100.0.5
  - @pnpm/config.writer@1100.0.6
  - @pnpm/worker@1100.1.2

## 1101.0.4

### Patch Changes

- @pnpm/config.writer@1100.0.5
- @pnpm/store.controller@1101.0.2

## 1101.0.3

### Patch Changes

- Updated dependencies [184ce26]
- Updated dependencies [6b891a5]
  - @pnpm/resolving.parse-wanted-dependency@1100.0.1
  - @pnpm/config.pick-registry-for-package@1100.0.2
  - @pnpm/resolving.npm-resolver@1101.0.1
  - @pnpm/store.controller-types@1100.0.4
  - @pnpm/fs.read-modules-dir@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.2
  - @pnpm/deps.graph-hasher@1100.1.2
  - @pnpm/store.controller@1101.0.2
  - @pnpm/config.writer@1100.0.4
  - @pnpm/network.fetch@1100.0.2
  - @pnpm/lockfile.utils@1100.0.4
  - @pnpm/worker@1100.1.1
  - @pnpm/installing.deps-resolver@1100.0.5
  - @pnpm/lockfile.types@1100.0.3
  - @pnpm/lockfile.fs@1100.0.4
  - @pnpm/lockfile.pruner@1100.0.3

## 1101.0.2

### Patch Changes

- @pnpm/store.controller@1101.0.1

## 1101.0.1

### Patch Changes

- @pnpm/config.writer@1100.0.3
- @pnpm/store.controller@1101.0.0

## 1101.0.0

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/worker@1100.1.0
  - @pnpm/store.controller@1101.0.0
  - @pnpm/store.controller-types@1100.0.3
  - @pnpm/resolving.npm-resolver@1101.0.0
  - @pnpm/installing.deps-resolver@1100.0.4
  - @pnpm/lockfile.utils@1100.0.3
  - @pnpm/deps.graph-hasher@1100.1.1
  - @pnpm/lockfile.fs@1100.0.3

## 1100.1.1

### Patch Changes

- Updated dependencies [c86c423]
- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/installing.deps-resolver@1100.0.3
  - @pnpm/deps.graph-hasher@1100.1.0
  - @pnpm/resolving.npm-resolver@1100.1.0
  - @pnpm/lockfile.types@1100.0.2
  - @pnpm/lockfile.utils@1100.0.2
  - @pnpm/store.controller@1100.0.2
  - @pnpm/store.controller-types@1100.0.2
  - @pnpm/lockfile.fs@1100.0.2
  - @pnpm/lockfile.pruner@1100.0.2
  - @pnpm/worker@1100.0.2
  - @pnpm/config.writer@1100.0.2

## 1100.1.0

### Minor Changes

- ea2a7fb: When pnpm is declared via the `packageManager` field in `package.json`, its resolution info is no longer written to `pnpm-lock.yaml` — unless the pinned pnpm version is v12 or newer. The `packageManagerDependencies` section is still populated (and reused across runs) when pnpm is declared via `devEngines.packageManager`. This makes the transition from pnpm v10 to v11 quieter by avoiding unnecessary lockfile churn for projects that pin an older pnpm in the legacy `packageManager` field.

### Patch Changes

- @pnpm/installing.deps-resolver@1100.0.2
- @pnpm/store.controller@1100.0.1

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/config.pick-registry-for-package@1100.0.1
  - @pnpm/config.writer@1100.0.1
  - @pnpm/core-loggers@1100.0.1
  - @pnpm/deps.graph-hasher@1100.0.1
  - @pnpm/installing.deps-resolver@1100.0.1
  - @pnpm/lockfile.fs@1100.0.1
  - @pnpm/lockfile.pruner@1100.0.1
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/lockfile.utils@1100.0.1
  - @pnpm/network.auth-header@1100.0.1
  - @pnpm/network.fetch@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.1
  - @pnpm/resolving.npm-resolver@1100.0.1
  - @pnpm/store.controller@1100.0.1
  - @pnpm/store.controller-types@1100.0.1
  - @pnpm/worker@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- 821b36a: Config dependencies are now installed into the global virtual store (`{storeDir}/links/`) and symlinked into `node_modules/.pnpm-config/`. This allows config dependencies to be shared across projects that use the same store, avoiding redundant fetches and imports.
- a8f016c: Store config dependency and package manager integrity info in `pnpm-lock.yaml` instead of inlining it in `pnpm-workspace.yaml`. The workspace manifest now contains only clean version specifiers for `configDependencies`, while the resolved versions, integrity hashes, and tarball URLs are recorded in the lockfile as a separate YAML document. The env lockfile section also stores `packageManagerDependencies` resolved during version switching and self-update. Projects using the old inline-hash format are automatically migrated on install.
- cc1b8e3: Fixed installation of config dependencies from private registries.

  Added support for object type in `configDependencies` when the tarball URL returned from package metadata differs from the computed URL [#10431](https://github.com/pnpm/pnpm/pull/10431).

- d8be970: Throws `FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` when attempting to install configuration dependencies with `--frozen-lockfile` active and the env lockfile is missing or out-of-date. Previously, the operation would silently rewrite the workspace file or resolve in-memory.
- 4a36b9a: Refactor workspace domains: rename `project-finder` to `projects-reader`, merge `filter-packages-from-dir` into `filter-workspace-packages`, and rename it to `projects-filter`. Also, move and rename `config/deps-installer` to `installing/env-installer`.

### Patch Changes

- Updated dependencies [5f73b0f]
- Updated dependencies [7721d2e]
- Updated dependencies [ae8b816]
- Updated dependencies [f98a2db]
- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [c55c614]
- Updated dependencies [a297ebc]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [5a0ed1d]
- Updated dependencies [7cec347]
- Updated dependencies [606f53e]
- Updated dependencies [831f574]
- Updated dependencies [0e9c559]
- Updated dependencies [e46a652]
- Updated dependencies [cd743ef]
- Updated dependencies [19f36cf]
- Updated dependencies [491a84f]
- Updated dependencies [94571fb]
- Updated dependencies [fb8962f]
- Updated dependencies [54c4fc4]
- Updated dependencies [e73da5e]
- Updated dependencies [61cad0c]
- Updated dependencies [b1ad9c7]
- Updated dependencies [50fbeca]
- Updated dependencies [2fc9139]
- Updated dependencies [19f36cf]
- Updated dependencies [0dfa8b8]
- Updated dependencies [121f64a]
- Updated dependencies [9eddabb]
- Updated dependencies [075aa99]
- Updated dependencies [c4045fc]
- Updated dependencies [143ca78]
- Updated dependencies [ba065f6]
- Updated dependencies [3bf5e21]
- Updated dependencies [6f361aa]
- Updated dependencies [0625e20]
- Updated dependencies [938ea1f]
- Updated dependencies [83fe533]
- Updated dependencies [2cb0657]
- Updated dependencies [bb8baa7]
- Updated dependencies [ee9fe58]
- Updated dependencies [d458ab3]
- Updated dependencies [021f70d]
- Updated dependencies [7d2fd48]
- Updated dependencies [9eddabb]
- Updated dependencies [144ce0e]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [780af09]
- Updated dependencies [50fbeca]
- Updated dependencies [bb8baa7]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [6c480a4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [cee1f58]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [4893853]
- Updated dependencies [10bc391]
- Updated dependencies [ba70035]
- Updated dependencies [3585d9a]
- Updated dependencies [38b8e35]
- Updated dependencies [394d88c]
- Updated dependencies [b7f0f21]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [2f98ec8]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [4f3ad23]
- Updated dependencies [09bb8db]
- Updated dependencies [9d3f00b]
- Updated dependencies [6557dc0]
- Updated dependencies [98a0410]
- Updated dependencies [efb48dc]
- Updated dependencies [6b3d87a]
  - @pnpm/installing.deps-resolver@1009.0.0
  - @pnpm/deps.graph-hasher@1003.0.0
  - @pnpm/config.writer@1001.0.0
  - @pnpm/store.controller-types@1005.0.0
  - @pnpm/resolving.npm-resolver@1005.0.0
  - @pnpm/worker@1001.0.0
  - @pnpm/store.controller@1005.0.0
  - @pnpm/constants@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.fs@1002.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/lockfile.utils@1004.0.0
  - @pnpm/config.pick-registry-for-package@1001.0.0
  - @pnpm/resolving.parse-wanted-dependency@1002.0.0
  - @pnpm/pkg-manifest.reader@1001.0.0
  - @pnpm/core-loggers@1002.0.0
  - @pnpm/fs.read-modules-dir@1001.0.0
  - @pnpm/network.auth-header@1001.0.0
  - @pnpm/lockfile.pruner@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/network.fetch@1001.0.0

## 1000.0.19

### Patch Changes

- Updated dependencies [6c3dcb8]
  - @pnpm/npm-resolver@1004.4.1
  - @pnpm/package-store@1004.0.0

## 1000.0.18

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/npm-resolver@1004.4.0
  - @pnpm/package-store@1004.0.0
  - @pnpm/config.config-writer@1000.0.14
  - @pnpm/pick-registry-for-package@1000.0.11
  - @pnpm/fetch@1000.2.6
  - @pnpm/core-loggers@1001.0.4
  - @pnpm/read-package-json@1000.1.2

## 1000.0.17

### Patch Changes

- @pnpm/package-store@1003.0.0

## 1000.0.16

### Patch Changes

- Updated dependencies [fb4da0c]
  - @pnpm/npm-resolver@1004.3.0
  - @pnpm/package-store@1002.0.12
  - @pnpm/config.config-writer@1000.0.13

## 1000.0.15

### Patch Changes

- Updated dependencies [baf8bf6]
- Updated dependencies [702ddb9]
  - @pnpm/npm-resolver@1004.2.3
  - @pnpm/package-store@1002.0.11

## 1000.0.14

### Patch Changes

- Updated dependencies [121b44e]
- Updated dependencies [02f8b69]
  - @pnpm/npm-resolver@1004.2.2
  - @pnpm/package-store@1002.0.11

## 1000.0.13

### Patch Changes

- @pnpm/error@1000.0.5
- @pnpm/npm-resolver@1004.2.1
- @pnpm/network.auth-header@1000.0.6
- @pnpm/read-package-json@1000.1.1
- @pnpm/config.config-writer@1000.0.12
- @pnpm/package-store@1002.0.11

## 1000.0.12

### Patch Changes

- Updated dependencies [e792927]
- Updated dependencies [38e2599]
- Updated dependencies [e792927]
  - @pnpm/read-package-json@1000.1.0
  - @pnpm/npm-resolver@1004.2.0
  - @pnpm/types@1000.8.0
  - @pnpm/config.config-writer@1000.0.11
  - @pnpm/pick-registry-for-package@1000.0.10
  - @pnpm/fetch@1000.2.5
  - @pnpm/core-loggers@1001.0.3
  - @pnpm/package-store@1002.0.10

## 1000.0.11

### Patch Changes

- Updated dependencies [87d3aa8]
  - @pnpm/fetch@1000.2.4
  - @pnpm/config.config-writer@1000.0.10
  - @pnpm/npm-resolver@1004.1.3
  - @pnpm/package-store@1002.0.9

## 1000.0.10

### Patch Changes

- Updated dependencies [adb097c]
  - @pnpm/read-package-json@1000.0.11
  - @pnpm/error@1000.0.4
  - @pnpm/npm-resolver@1004.1.3
  - @pnpm/config.config-writer@1000.0.9
  - @pnpm/package-store@1002.0.9
  - @pnpm/network.auth-header@1000.0.5

## 1000.0.9

### Patch Changes

- Updated dependencies [1a07b8f]
  - @pnpm/types@1000.7.0
  - @pnpm/config.config-writer@1000.0.8
  - @pnpm/pick-registry-for-package@1000.0.9
  - @pnpm/fetch@1000.2.3
  - @pnpm/core-loggers@1001.0.2
  - @pnpm/read-package-json@1000.0.10
  - @pnpm/npm-resolver@1004.1.2
  - @pnpm/package-store@1002.0.8
  - @pnpm/error@1000.0.3
  - @pnpm/network.auth-header@1000.0.4

## 1000.0.8

### Patch Changes

- @pnpm/config.config-writer@1000.0.7
- @pnpm/npm-resolver@1004.1.1
- @pnpm/package-store@1002.0.7

## 1000.0.7

### Patch Changes

- @pnpm/package-store@1002.0.6

## 1000.0.6

### Patch Changes

- Updated dependencies [2721291]
  - @pnpm/npm-resolver@1004.1.0
  - @pnpm/package-store@1002.0.5
  - @pnpm/config.config-writer@1000.0.6

## 1000.0.5

### Patch Changes

- @pnpm/package-store@1002.0.4

## 1000.0.4

### Patch Changes

- 09cf46f: Update `@pnpm/logger` in peer dependencies.
- Updated dependencies [51bd373]
- Updated dependencies [09cf46f]
- Updated dependencies [5ec7255]
  - @pnpm/network.auth-header@1000.0.3
  - @pnpm/npm-resolver@1004.0.1
  - @pnpm/core-loggers@1001.0.1
  - @pnpm/package-store@1002.0.3
  - @pnpm/fetch@1000.2.2
  - @pnpm/types@1000.6.0
  - @pnpm/config.config-writer@1000.0.5
  - @pnpm/pick-registry-for-package@1000.0.8
  - @pnpm/read-package-json@1000.0.9

## 1000.0.3

### Patch Changes

- @pnpm/config.config-writer@1000.0.4
- @pnpm/package-store@1002.0.2

## 1000.0.2

### Patch Changes

- Updated dependencies [8a9f3a4]
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
  - @pnpm/parse-wanted-dependency@1001.0.0
  - @pnpm/npm-resolver@1004.0.0
  - @pnpm/core-loggers@1001.0.0
  - @pnpm/logger@1001.0.0
  - @pnpm/types@1000.5.0
  - @pnpm/package-store@1002.0.2
  - @pnpm/fetch@1000.2.1
  - @pnpm/config.config-writer@1000.0.3
  - @pnpm/pick-registry-for-package@1000.0.7
  - @pnpm/read-package-json@1000.0.8

## 1000.0.1

### Patch Changes

- Updated dependencies [81f441c]
- Updated dependencies [17b7e9f]
  - @pnpm/npm-resolver@1003.0.0
  - @pnpm/config.config-writer@1000.0.2
  - @pnpm/package-store@1002.0.1

## 1000.0.0

### Major Changes

- 1413c25: Initial release.

### Minor Changes

- 750ae7d: Now you can use the `pnpm add` command with the `--config` flag to install new configurational dependencies [#9377](https://github.com/pnpm/pnpm/pull/9377).

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [72cff38]
- Updated dependencies [750ae7d]
- Updated dependencies [750ae7d]
  - @pnpm/types@1000.4.0
  - @pnpm/npm-resolver@1002.0.0
  - @pnpm/package-store@1002.0.0
  - @pnpm/core-loggers@1000.2.0
  - @pnpm/fetch@1000.2.0
  - @pnpm/config.config-writer@1000.0.1
  - @pnpm/pick-registry-for-package@1000.0.6
  - @pnpm/read-package-json@1000.0.7
