---
"@pnpm/config.reader": minor
"@pnpm/store.index": minor
"@pnpm/store.controller": minor
"@pnpm/store.connection-manager": minor
"@pnpm/building.after-install": patch
"@pnpm/worker": minor
"@pnpm/installing.package-requester": minor
"@pnpm/installing.context": patch
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added a new setting `frozenStore` (`--frozen-store`) that lets `pnpm install` run against a package store on a read-only filesystem (e.g. a Nix store, a read-only bind mount, an OCI layer). When enabled, pnpm opens the store's SQLite `index.db` through the `immutable=1` URI — bypassing the WAL/`-shm` sidecar creation that otherwise fails on a read-only directory — and suppresses every store-write path (the `index.db` writer and the project-registry write). Pair it with `--offline --frozen-lockfile` against a fully-populated store. Incompatible with `--force` and with a configured pnpr server, since both write into the store.
