# @pnpm/agent.client

## 1.1.0

### Minor Changes

- 5192edf: The pnpr install accelerator now forwards the caller's per-registry credentials on `POST /v1/install`, so it can resolve, verify, and fetch private dependencies from external registries as the caller. The client sends an `Authorization` header identifying itself to the pnpr server plus an `authHeaders` map of the registry tokens (built with `@pnpm/network.auth-header`), and the server threads those credentials through resolution and fetch instead of reaching the registry anonymously. Externally-resolved private content carries no pnpr access policy, so the server gates it per user against the owning registry — serving a cache hit only to a user the registry has cleared — and re-checks access (clearing it on a `401`/`403`) rather than letting the store's possession of the bytes authorize anyone. Packages the registry serves anonymously are classified public once (globally) and then served to everyone without per-user access checks, so a registry that mixes public and private packages doesn't pay the per-user cost for its public ones.
- f429f93: `pnpm install --lockfile-only` (and the `lockfileOnly` setting) is now honored when a `pnprServer` is configured. The pnpr path resolves and writes `pnpm-lock.yaml` but fetches no files into the store and links no `node_modules`, matching the local lockfile-only behavior. The client ignores any file/index lines an older pnpr server still streams, so the store stays untouched even against a server that predates the resolve-only mode [#12146](https://github.com/pnpm/pnpm/issues/12146).
- a017bf3: Renamed the experimental `agent` setting to `pnprServer` so the pnpm CLI matches the same setting name pacquet uses for offloading resolution to a [pnpr](https://github.com/pnpm/pnpm/tree/main/pnpr) server. Point pnpm at a pnpr server with `pnprServer: <url>` in `pnpm-workspace.yaml` (or `--pnpr-server <url>`); the previous `agent` / `--agent` name no longer works. The client package was likewise renamed from `@pnpm/agent.client` to `@pnpm/pnpr.client`.

### Patch Changes

- a017bf3: Fixed `optionalDependencies` being dropped when resolving through a `pnprServer`. The pnpr request now carries each project's optional dependencies (for both single-project and workspace installs), so the server resolves them like the local resolver does instead of producing a lockfile as if they did not exist.
- 3b76b8e: The pnpr install accelerator now serves resolved files only in the single gzipped `POST /v1/install` response and authorizes every package whose bytes it serves against the server's access policy. The separate unauthenticated `POST /v1/files` endpoint has been removed: the client materializes the inlined files straight into its content-addressable store, and a content-addressed digest is no longer a bearer capability for a package the caller cannot read.
- Updated dependencies [3b76b8e]
  - @pnpm/worker@1100.1.9
  - @pnpm/lockfile.fs@1100.1.3
  - @pnpm/lockfile.types@1100.0.9
  - @pnpm/store.cafs@1100.1.8

## 1.0.8

### Patch Changes

- Updated dependencies [aa6149d]
  - @pnpm/worker@1100.1.8
  - @pnpm/lockfile.types@1100.0.8
  - @pnpm/store.cafs@1100.1.7

## 1.0.7

### Patch Changes

- @pnpm/lockfile.types@1100.0.7
- @pnpm/store.cafs@1100.1.6
- @pnpm/worker@1100.1.7

## 1.0.6

### Patch Changes

- @pnpm/lockfile.types@1100.0.6
- @pnpm/store.cafs@1100.1.5
- @pnpm/worker@1100.1.6

## 1.0.5

### Patch Changes

- @pnpm/store.cafs@1100.1.4
- @pnpm/worker@1100.1.5

## 1.0.4

### Patch Changes

- @pnpm/lockfile.types@1100.0.5
- @pnpm/store.cafs@1100.1.3
- @pnpm/worker@1100.1.4

## 1.0.3

### Patch Changes

- Updated dependencies [0c67cb5]
  - @pnpm/store.index@1100.1.0
  - @pnpm/worker@1100.1.3

## 1.0.2

### Patch Changes

- Updated dependencies [27425d7]
  - @pnpm/lockfile.types@1100.0.4
  - @pnpm/store.cafs@1100.1.2
  - @pnpm/worker@1100.1.2

## 1.0.1

### Patch Changes

- @pnpm/worker@1100.1.1
- @pnpm/lockfile.types@1100.0.3
- @pnpm/store.cafs@1100.1.1

## 1.0.0

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/store.cafs@1100.1.0
  - @pnpm/worker@1100.1.0

## 0.0.1

### Patch Changes

- @pnpm/lockfile.types@1100.0.2
- @pnpm/store.cafs@1100.0.2
- @pnpm/worker@1100.0.2
