---
"@pnpm/config.reader": minor
"@pnpm/store.index": minor
"@pnpm/store.controller": minor
"@pnpm/store.connection-manager": minor
"@pnpm/building.after-install": patch
"@pnpm/building.during-install": patch
"@pnpm/worker": minor
"@pnpm/installing.package-requester": minor
"@pnpm/installing.context": patch
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added a new setting `frozenStore` (`--frozen-store`) that lets `pnpm install` run against a package store on a read-only filesystem (e.g. a Nix store, a read-only bind mount, an OCI layer). When enabled, pnpm opens the store's SQLite `index.db` through the `immutable=1` URI — bypassing the WAL/`-shm` sidecar creation that otherwise fails on a read-only directory — and suppresses every store-write path (the `index.db` writer and the project-registry write). Pair it with `--offline --frozen-lockfile` against a fully-populated store. Under the global virtual store, package directories live inside the store, so if the store is missing the build output of a package whose lifecycle scripts are approved (or that has a patch), pnpm fails up front with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` rather than crashing mid-build on a read-only write — seed the store with those builds first. Incompatible with `--force` and with a configured pnpr server, since both write into the store. The read-only `immutable=1` open requires Node.js >=22.15.0 (or >=24); on older runtimes `--frozen-store` fails with a clear `ERR_PNPM_FROZEN_STORE_UNSUPPORTED_NODE` error.
