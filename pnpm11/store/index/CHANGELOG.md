# @pnpm/store.index

## 1100.2.1

### Patch Changes

- Updated dependencies [852d537]
  - @pnpm/error@1100.0.1

## 1100.2.0

### Minor Changes

- 61810aa: Added a new setting `frozenStore` (`--frozen-store`) that lets `pnpm install` run against a package store on a read-only filesystem (e.g. a Nix store, a read-only bind mount, an OCI layer). When enabled, pnpm opens the store's SQLite `index.db` through the `immutable=1` URI — bypassing the WAL/`-shm` sidecar creation that otherwise fails on a read-only directory — and suppresses every store-write path (the `index.db` writer and the project-registry write). Pair it with `--offline --frozen-lockfile` against a fully-populated store. Under the global virtual store, package directories live inside the store, so if the store is missing the build output of a package whose lifecycle scripts are approved (or that has a patch), pnpm fails up front with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` rather than crashing mid-build on a read-only write — seed the store with those builds first. Incompatible with `--force` and with a configured pnpr server, since both write into the store; the side-effects cache is likewise not written under `frozenStore`. If the store is missing its content directory, the install fails fast with `ERR_PNPM_FROZEN_STORE_INCOMPLETE` rather than attempting to initialize it. The read-only `immutable=1` open requires Node.js >=22.15.0, >=23.11.0, or >=24.0.0; on older runtimes `--frozen-store` fails with a clear `ERR_PNPM_FROZEN_STORE_UNSUPPORTED_NODE` error. Bin-linking also tolerates a read-only store: under the global virtual store a package's bin source lives inside the store, so the `chmod` that makes it executable would be refused — with `EPERM`/`EACCES`, or with `EROFS` on a genuinely read-only filesystem. That `chmod` is redundant when the seed already ships its bins executable with a normalized shebang, so it is now skipped in that case, while a non-executable bin (or one still carrying a Windows CRLF shebang) on a read-only store still errors.

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

## 1100.1.0

### Minor Changes

- 0c67cb5: Export `pickStoreIndexKey(resolution, pkgId, { built })` — picks the appropriate store-index key for a resolution:
  git-hosted entries route through `gitHostedStoreIndexKey(pkgId, { built })`, everything else through
  `storeIndexKey(resolution.integrity, pkgId)`. Centralizes the routing for `installing.package-requester`,
  `building.after-install`, `store.pkg-finder`, and `modules-mounter.daemon` so each consumer reads
  `resolution.gitHosted` once via a single typed call.

## 1000.0.0

### Minor Changes

- b7f0f21: Use SQLite for storing package index in the content-addressable store. Instead of individual `.mpk` files under `$STORE/index/`, package metadata is now stored in a single SQLite database at `$STORE/index.db`. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10826](https://github.com/pnpm/pnpm/issues/10826).
