# Node API bindings for pacquet (`@pnpm/napi`)

## Purpose

Expose pnpm v12's Rust engine (pacquet) to Node.js hosts through a NAPI addon, so
programmatic consumers of pnpm — Bit being the reference consumer — can run installs,
resolution, rebuilds, peer-dependency checks, and pack through the Rust implementation
instead of the TypeScript `@pnpm/*` packages published from `pnpm11/`.

The binding replaces only the **engine** surface. Pure data utilities that operate on
in-memory objects (`@pnpm/types`, `@pnpm/lockfile.types`, `@pnpm/lockfile.fs` object
transforms, `@pnpm/deps.path`, `@pnpm/installing.modules-yaml`,
`@pnpm/deps.inspection.*`, `@pnpm/config.reader`, `@pnpm/cli.default-reporter`,
`@pnpm/logger`) remain valid JS packages: both stacks keep the same on-disk contract
(lockfile v9 byte-stability, `.modules.yaml`, store layout v11), so JS-side reads and
in-memory transforms stay correct while Rust owns all engine I/O.

## Deliverables

1. **Rust crate** `pacquet-napi` at `pnpm/crates/napi`
   - `crate-type = ["cdylib"]`, napi-rs v3 (`napi` + `napi-derive`, `tokio_rt` feature).
   - New workspace profile `[profile.napi-release]` (inherits `release`,
     `panic = "unwind"`) — the workspace release profile uses `panic = "abort"`,
     which would take down the host Node process on any Rust panic.
2. **npm wrapper package** `@pnpm/napi` at `pnpm/npm/napi`
   - Standard napi-rs loader (`index.js` resolves the platform `.node` from
     optionalDependencies `@pnpm/napi.<platform>`, falling back to a local build),
     hand-written `index.d.ts`.
   - Version line `1200.0.0` (v12 engine, matching the `NN00` convention of
     `@pnpm/*` package versions).
   - Platform packages follow the same 8-target matrix as `pnpm/npm/pnpm`
     (`scripts/generate-packages.mjs`): win32-x64/arm64, darwin-x64/arm64,
     linux-x64/arm64 gnu + musl.

## Exported API

All functions are `async` (napi tokio) unless noted. Complex inputs/outputs cross the
boundary as plain JS objects (serde_json round-trip), matching the shapes of the
corresponding pnpm v11 TS APIs so the consumer-side diff stays minimal.

### `install(options): Promise<InstallResult>`

The equivalent of `mutateModules(importers, opts)` from
`@pnpm/installing.deps-installer`, restricted to the `install` mutation.

```ts
interface NodeApiProject {
  rootDir: string
  manifest: PackageManifest        // in-memory; NOT read from disk
}
interface InstallOptions {
  dir: string                      // lockfile/workspace root
  projects: NodeApiProject[]       // importers, in-memory manifests
  // --- engine config (maps onto pacquet_config::Config overlay) ---
  storeDir?: string
  cacheDir?: string
  registries?: Record<string, string>     // { default: url, '@scope': url }
  authConfig?: Record<string, string>     // raw nerf-darted .npmrc auth entries
  proxyConfig?: { httpProxy?, httpsProxy?, noProxy? }
  networkConfig?: { ca?, cert?, key?, localAddress?, strictSsl?, maxSockets?,
    networkConcurrency?, fetchRetries?, fetchRetryFactor?, fetchRetryMintimeout?,
    fetchRetryMaxtimeout?, fetchTimeout?, userAgent? }
  nodeLinker?: 'hoisted' | 'isolated'
  hoistPattern?: string[]
  publicHoistPattern?: string[]
  externalDependencies?: string[]        // already supported by pacquet config/hoisting
  overrides?: Record<string, string>
  allowBuilds?: Record<string, boolean>  // + dangerouslyAllowAllBuilds
  dangerouslyAllowAllBuilds?: boolean
  autoInstallPeers?: boolean
  excludeLinksFromLockfile?: boolean
  lockfileOnly?: boolean
  frozenLockfile?: boolean
  preferFrozenLockfile?: boolean
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone'
  preferOffline?: boolean
  virtualStoreDirMaxLength?: number
  peersSuffixMaxLength?: number
  dedupePeerDependents?: boolean
  dedupeDirectDeps?: boolean
  dedupeInjectedDeps?: boolean
  injectWorkspacePackages?: boolean
  hoistWorkspacePackages?: boolean
  enableModulesDir?: boolean
  ignorePackageManifest?: boolean
  nodeVersion?: string
  engineStrict?: boolean
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  neverBuiltDependencies?: string[]
  update?: boolean                       // updateAll → depth Infinity
  depth?: number
  includeOptionalDeps?: boolean
  // --- host callbacks ---
  readPackageHook?: (manifest: object) => object
  onLog?: (event: object) => void        // reporter bridge, see below
}
interface InstallResult {
  stats: { added: number; removed: number; linkedToRoot: number }
  depsRequiringBuild?: string[]
  storeDir: string
}
```

Implementation notes:
- Build a `pacquet_config::Config` starting from `Config::current` over `options.dir`,
  then overlay the explicit option fields. Intern the leaked `&'static Config` in a
  `DashMap<ConfigKey, &'static Config>` keyed by a hash of (dir, overlay) so repeated
  installs in one process don't leak unboundedly (`Config::leak` is one-way).
- Feed `options.projects` through a new **programmatic importer source**: extend the
  install pipeline so `importer_manifests` (see
  `install_with_fresh_lockfile.rs:989-1196`) can come from caller-supplied
  `(id, manifest)` pairs instead of on-disk workspace discovery. This is the one real
  pacquet-side feature addition; it mirrors what the TS `mutateModules` has always
  accepted (`allProjects` with in-memory manifests).
- Build ordering: Rust's `graph_sequencer`/`build_sequence` handles lifecycle ordering;
  callers do not pass `buildIndex` (Bit's `groupPkgs`/`sortProjects` dance is dropped).
- `readPackageHook` maps onto the existing `PnpmfileHooks` seam via a `ThreadsafeFunction`
  that serializes the manifest to JSON and deserializes the synchronous JS result. The JS
  side receives `(manifest)` for dependency manifests. Importer-manifest transforms that
  need `workspaceDir` stay on the host side before calling the binding.
- Run each install on a dedicated tokio runtime thread with a 32 MiB stack (same
  rationale as `pacquet_cli::main`), and lazily init the global rayon pool exactly like
  the CLI (`configure_rayon_pool`).
- Serialize concurrent installs per `dir` inside the addon (mirror of Bit's
  `installsRunning` map) to protect the lockfile/virtual store.

### `rebuild(options): Promise<void>`

Wraps `Install::run_rebuild` (`RebuildOptions`; `pending`,
`skipIfHasSideEffectsCache`). Replaces `@pnpm/building.commands` `rebuild.handler`.

### `getPeerDependencyIssues(options): Promise<PeerDependencyIssuesByProjects>`

Runs resolution only (`resolve_workspace` + `resolve_peers_workspace`) and returns the
per-project peer issues in the same JSON shape as
`@pnpm/installing.deps-installer`'s `getPeerDependencyIssues`
(`{ [projectId]: { missing, bad, conflicts, intersections } }`).

### `resolveDependency(wanted, options): Promise<ResolveResult>`

The equivalent of `createResolver(...)` + `resolve(wantedDep, opts)` from
`@pnpm/installing.client`, backed by `resolving-default-resolver` (npm, git, tarball,
local, jsr). Input `{ alias?, bareSpecifier? }` + `{ dir, registries, authConfig,
proxyConfig, networkConfig, cacheDir, fullMetadata? }`; output
`{ manifest, resolvedVia, normalizedBareSpecifier, id, latest? }`. Metadata cache is
the in-process `InMemoryPackageMetaCache`, shared per (registry, cacheDir).

### `pack(options): Promise<PackResult>`

Wraps `pacquet_pack::api` with `SilentReporter` + `Host` capabilities. Replaces the
`@pnpm/releasing.commands` internal `publish/pack.js` side-load. Sync Rust, exposed
async. Returns `{ publishedManifest, contents, tarballPath, unpackedSize }`.

### `parseBareSpecifier(spec, alias?): ParsedBareSpecifier | null` (sync)

Pure validation/parse helper replacing `@pnpm/resolving.npm-resolver`'s
`parseBareSpecifier` usage.

### Reporter bridge (`onLog`)

`crates/reporter`'s `Reporter` trait is static-dispatch (`fn emit(event: &LogEvent)`,
no `&self`), so the addon defines `NodeBridgeReporter` whose `emit` forwards
`serde_json::to_value(event)` through a process-global
`OnceLock<ThreadsafeFunction<serde_json::Value>>` (non-blocking enqueue; events may
fire from rayon and tokio threads — same constraint documented on the trait). pacquet's
`LogEvent` stream is wire-compatible with `@pnpm/core-loggers`, so a JS host can pipe
events straight into `@pnpm/logger`'s `streamParser` and keep rendering with
`@pnpm/cli.default-reporter` — which is exactly what Bit does (custom
`filterPkgsDiff`, `approveBuildsInstructionText` keep working untouched). One global
sink matches the JS reality (pnpm's logger is process-global there too).

### Error mapping

All exported functions catch the pacquet `Diagnostic` error enums and re-throw JS
errors shaped like `PnpmError`: `{ code: 'ERR_PNPM_*', message, hint?, pkgsStack? }`.
Consumers keep their existing `PnpmError → host error` translation (Bit:
`pnpm-error-to-bit-error.ts`). Panics are caught by napi-rs (unwind profile) and
surface as generic `Error`s rather than aborting the host.

## What stays TypeScript in consumers

| Kept JS package | Why |
|---|---|
| `@pnpm/logger`, `@pnpm/cli.default-reporter` | render the LogEvent stream (UI only) |
| `@pnpm/lockfile.fs` / `.types` / `.filtering` | lockfile v9 is byte-stable across stacks; reads and in-memory transforms are engine-independent |
| `@pnpm/deps.path` | pure dep-path string parsing |
| `@pnpm/installing.modules-yaml` | reads `.modules.yaml` (same format both stacks) |
| `@pnpm/deps.inspection.*` | dependents tree = pure lockfile analysis |
| `@pnpm/config.reader` (+ nerf-dart, parse-overrides, ca-file) | host-side config/auth introspection; engine gets explicit options |
| `@pnpm/types`, `@pnpm/error` | type-only |
| `@pnpm/node-fetch`, `@pnpm/semver-diff`, `@pnpm/colorize-semver-diff`, `@pnpm/registry-mock`, `@pnpm/plugin-trusted-deps` | unrelated to the engine |

Dropped from consumers: `@pnpm/installing.deps-installer`, `@pnpm/installing.client`,
`@pnpm/store.connection-manager`, `@pnpm/store.controller`, `@pnpm/building.commands`,
`@pnpm/worker`, `@pnpm/workspace.projects-graph`, `@pnpm/workspace.projects-sorter`,
`@pnpm/releasing.commands`, `@pnpm/resolving.npm-resolver`.

## Implementation status

- **Done and verified** (real engine, smoke-tested through a built `.node`):
  - `pack`, `parseBareSpecifier`, `engineVersion`, the structured error envelope
    (`code` / `hint` lifted onto the thrown JS `Error` by the wrapper's
    `index.js`), and the reporter bridge (`NodeBridgeReporter` +
    `ThreadsafeFunction` sink, exercised via `pack`/`install` `onLog`).
  - The `&'static Config` overlay + interning (`config.rs`): base
    `Config::current::<Host>(dir)` with the host's explicit fields layered on,
    leaked once and cached in a `DashMap` keyed by a hash of `(dir, overlay)`.
  - **`install` for a single importer** — end-to-end verified: a real
    `is-odd@3.0.1` install resolved + fetched its transitive `is-number`, linked
    an isolated `node_modules`, wrote `pnpm-lock.yaml`, returned
    `stats.added == 2`, and a second call was correctly idempotent
    (`added == 0`). Runs `pacquet_package_manager::Install` on a dedicated
    32 MiB-stack worker thread with its own multi-thread tokio runtime; the napi
    async fn awaits the outcome over a oneshot channel so pacquet's borrowed
    `State` never crosses the FFI boundary. `install` calls are serialized by a
    global lock so the reporter-driven stats accumulator stays correct.
  - Small pacquet addition: `PackageManifest::from_value(path, value)` — build an
    in-memory manifest without touching disk (applies the same
    `engines.runtime` normalization as a disk read).

- **`readPackage` hook bridge — DONE and verified.** An
  `Option<Arc<dyn PnpmfileHooks>>` override is plumbed through `Install` →
  `InstallWithFreshLockfile` (`pnpmfile_hook_override`, preferred over
  `finder::load_pnpmfile` on the fresh-resolve path). The binding's
  `JsReadPackageHook` (`hooks.rs`) adapts a **synchronous** JS
  `(manifest) => manifest` callback via a `ThreadsafeFunction::call_async`,
  invoked per resolved dependency manifest. Promise-returning hooks are rejected by the
  TypeScript contract rather than being silently ignored. Verified: installing `is-odd`
  (deps on `is-number`) with a hook that strips `is-number` produced
  `added: 1` and no `is-number` on disk. Contract gap to keep in mind:
  pacquet's `PnpmfileHooks::read_package(pkg, ctx)` passes **no `workspaceDir`**
  (`crates/hooks/src/lib.rs`), which several Bit hooks use — so
  importer-manifest transforms are pre-applied JS-side in `lynx.ts`, leaving the
  Rust hook to handle the dependency-manifest transforms (strip legacy/harmony).

- **Multiple importers — DONE and verified.** `Install` gained
  `workspace_projects_override: Option<Vec<pacquet_workspace::Project>>`;
  `run_inner` uses it instead of `load_workspace_projects` (disk
  `pnpm-workspace.yaml` discovery) when set. The root importer stays
  `Install.manifest`; every importer (root included) feeds the `workspace:`-spec
  lookup. Verified: a workspace where member `a` depends on member `b` via
  `workspace:*` (plus a registry dep) linked `a/node_modules/@ws/b` to the local
  `b` dir, installed the registry dep, and wrote a lockfile with `packages/a:` /
  `packages/b:` importer entries. The binding builds the override from the
  caller's `projects` (single importer → `None`, the plain non-workspace path).

- **Build-script approval — DONE and verified.** The overlay wires
  `strict_dep_builds` (defaulted **off** in the binding — an install reports
  blocked builds in `InstallResult.depsRequiringBuild` instead of failing with
  `ERR_PNPM_IGNORED_BUILDS`, matching how Bit gates builds itself), plus
  `allow_builds` (per-package allow-list) and `dangerously_allow_all_builds`.
  Verified: installing `es5-ext@0.10.64` reported it in `depsRequiringBuild`
  and did not fail; re-running with `allowBuilds: { 'es5-ext': true }` actually
  ran its build script (lifecycle events fired) and dropped it from
  `depsRequiringBuild`.

- **`rebuild` — DONE and verified.** Shares `install`'s State/Install
  construction (an `EngineMode` picks `frozen_lockfile: true` +
  `is_full_install: false` and calls `Install::run_rebuild`). `selectedNames`
  maps to `RebuildOptions::selected_names` (empty/omitted → rebuild every
  build-needing package). Verified: rebuild runs the frozen path against a
  materialized install and emits the expected event stream.

- **Install-option coverage — DONE.** The options the binding used to reject
  with `ERR_PNPM_NAPI_UNSUPPORTED_OPTION` now flow through the engine
  (pnpm/pnpm#12823). Only `authConfig` (use `authHeaderByUri` instead) and
  `neverBuiltDependencies` remain rejected.
  - `update` → `UpdateSeedPolicy::DropAll` (whole-graph re-resolve to
    highest-in-range); `prefer_frozen_lockfile` / frozen fast paths are forced
    off so the re-resolution runs. `depth` is accepted but, without package
    selectors, is a no-op toggle.
  - `engineStrict` / `nodeVersion` → two new `pacquet_config::Config` fields
    (also parsed from `pnpm-workspace.yaml` / `PNPM_CONFIG_*`), threaded into
    `InstallabilityHost` (fresh + frozen paths) via `detect_with`. An explicit
    `nodeVersion` is authoritative (no `node --version` probe).
  - `maxSockets` → a per-origin socket cap on `ThrottledClient`
    (`with_max_sockets_per_host`), mirroring undici's per-origin `connections`;
    the global `networkConcurrency` semaphore stays the outer bound.
  - `enableModulesDir: false` → pacquet's lockfile-only path (resolve + write
    lockfile, materialize no `node_modules`).
  - `ignorePackageManifest` → pacquet's existing `ignore_manifest_check` (skip
    the manifest↔lockfile freshness gate). pnpm additionally skips the
    project-level linking phase (`pnpm fetch` semantics); a fuller native port
    of that is a follow-up.
  - `pnpmHomeDir` → accepted and ignored; only global flows consult it and the
    binding drives project installs.

- **`install` remaining work** (additive; core pipeline + hook + multi-importer
  + build approval above are proven):
  1. **Auth / private registries** — build `config.auth_headers` /
     `tls_by_uri` from Bit's raw nerf-darted `authConfig` (the verifications
     used the public registry, which needs none).
  2. **`stats.linkedToRoot`** — pacquet has no separate emit; consumers use
     `added + removed` for "did anything change".

- **`resolveDependency` — DONE and verified (npm registry).** Assembles an
  `NpmResolver` from the config overlay (shared `InMemoryPackageMetaCache` /
  fetch-locker / picked-manifest caches, `ThrottledClient`, `resolved_registries`
  so the `default` route is present) and calls `Resolver::resolve`. Verified:
  `is-odd@^3.0.0` → `3.0.1` via `npm-registry`, `@latest` → `3.0.1` with the
  `latest` tag, an exact spec returns the full manifest (`dependencies` intact),
  and a `git+https://…` specifier the npm resolver doesn't claim returns a clear
  error. Non-npm protocols (git / tarball / local) need the rest of the
  default-resolver chain wired — a follow-up.

- **Present but stubbed** (export exists so the JS contract and consumers are
  type-stable; returns `ERR_PNPM_NAPI_UNIMPLEMENTED`):
  `getPeerDependencyIssues` — runs full-tree resolve + `resolve_peers` and
  reports peer conflicts without linking; the diagnostic op Bit uses least.

## Consumer (Bit) integration status

Bit's `pnpm11-rust` branch is rewired to `@pnpm/napi` and **`npm run lint`
(`tsc --noEmit` + oxlint) passes clean (0 errors, 0 warnings)**:

- `scopes/dependencies/pnpm/lynx.ts` — `install` → `nodeApi.install`
  (in-memory `projects`, importer manifests pre-transformed by Bit's hooks with
  their `workspaceDir`, dependency manifests transformed via the synchronous
  `readPackageHook`); `resolveRemoteVersion` → `nodeApi.resolveDependency`;
  the `rebuild` closure → `nodeApi.rebuild`; `getPeerDependencyIssues` calls
  the stubbed export and returns `{}` on `ERR_PNPM_NAPI_UNIMPLEMENTED`.
  Reporter events bridge via `streamParser.emit('data', event)` into the kept
  `@pnpm/cli.default-reporter`. `peerDependencyRules`,
  `resolvePeersFromWorkspaceRoot`, and `preferFrozenLockfile` are forwarded.
- `scopes/pkg/pkg/packer.ts` → `nodeApi.pack`; `load-pnpm-pack.cjs` deleted.
- `parseBareSpecifier` (dependency-resolver runtime), `PeerDependencyIssuesByProjects`
  type, and the `pnpm-error-to-bit-error` converter (duck-typed on `.code`/`.hint`)
  all moved to `@pnpm/napi`.
- `workspace.jsonc` drops the 10 engine packages (`@pnpm/installing.deps-installer`,
  `installing.client`, `store.connection-manager`, `store.controller`,
  `building.commands`, `worker`, `workspace.projects-graph`,
  `workspace.projects-sorter`, `releasing.commands`, `resolving.npm-resolver`)
  and adds `@pnpm/napi`. Data/format packages (`lockfile.*`, `deps.path`,
  `installing.modules-yaml`, `deps.inspection.*`, `config.reader`, `logger`,
  `cli.default-reporter`, `types`, `error`, …) stay.

### Auth / private registries — DONE

Bit installs `@teambit/*` from a private registry. Auth is now wired end-to-end:
the binding accepts `authHeaderByUri` (a `SharedEngineOptions` field) — a map of
nerf-darted registry URI → `Authorization` header value, with `""` for the
default registry — and applies it via `AuthHeaders::from_creds_map`, replacing
the `.npmrc`-derived `config.auth_headers`. Bit computes the header map in
`lynx.ts` (`buildAuthHeaderByUri`) from its `authConfig` using the kept
`@pnpm/config.reader` (`getNetworkConfigs` / `getDefaultCreds`) — `_authToken` →
`Bearer …`, `_auth` (username/password) → `Basic …` — so npmrc auth is parsed
once, JS-side, and never reimplemented in Rust. Wired into both `install` and
`resolveDependency`. Smoke-tested: the binding accepts and applies the header
map without error; `npm run lint` stays clean.

### Runtime-verified through Bit's actual code

Bit's production `lynx.ts` was exercised through `babel-register` (the transpile
path Bit's e2e tests use), driving the real Rust engine:

- **`lynx.install`** — a real `is-odd@3.0.1` install: linked into `node_modules`,
  imports and runs, `dependenciesChanged: true`, `pnpm-lock.yaml` written.
- **Reporter bridge** — with output enabled, `@pnpm/cli.default-reporter`
  rendered live pnpm-style progress ("resolved 2, downloaded 2, added 2, done",
  the `+2` summary, and the lockfile supply-chain-policy line) from the engine's
  events via `streamParser.emit('data', event)`.
- **`rebuild()`** and **`resolveRemoteVersion`** (`is-odd@^3.0.0` → `3.0.1` via
  `npm-registry`) both work through the real Bit code path.

### Distribution — DONE

`pnpm/npm/napi/scripts/generate-packages.mjs` produces the eight
`@pnpm/napi.<platform>` prebuilt packages (`win32`/`darwin`/`linux` ×
`x64`/`arm64`, plus musl on Linux) and wires them as the wrapper's
`optionalDependencies` — the same model as `@pnpm/exe.*`. CI cross-compiles the
addon per target (`napi build --release --target <rust-triple>`), uploads each
as `pnpm-napi.<codeTarget>.node` at the repo root, then runs the generator.
The wrapper's `index.js` resolves `@pnpm/napi.<triple>` at load time (env
override → platform package → local build). Verified: the generator emits a
correct `darwin-arm64` platform package whose `.node` loads as the addon; a
`README.md` documents the API, distribution, and local-dev build. Generated
platform packages and cross-compiled artifacts are gitignored.

### Remaining

1. **`getPeerDependencyIssues`** — the one intentional stub. It requires
   assembling `resolve_workspace` standalone (`WorkspaceResolveOptions` +
   per-importer `ResolveImporterOptions` + a resolver chain) to surface
   `peer_dependency_issues_by_importer`. Deferred: it's the least-used
   diagnostic op, Bit degrades gracefully (returns `{}`), and pacquet's own CLI
   doesn't render peer issues yet either ("issue renderer not ported yet").
2. **Full `bit install` via the CLI** in a real Bit workspace — the `lynx`
   engine seam is runtime-proven (see above); the remaining gap is driving it
   through a whole `bit` command against Bit's workspace + registry.

## Open items tracked during implementation

- `bit`-namespaced passthrough at the lockfile top level: the Rust `Lockfile` struct
  must not drop unknown top-level keys it round-trips (consumers persist custom
  attributes, e.g. Bit's `bit.depsRequiringBuild`). Add a `#[serde(flatten)]`
  passthrough map preserved by the YAML emitter.
- `depsRequiringBuild` in `InstallResult`: surface the ignored-builds list the engine
  already computes for `ERR_PNPM_IGNORED_BUILDS` / `pnpm:ignored-scripts`.
- `modulesCacheMaxAge: Infinity` semantics (consumer prunes the virtual store itself):
  confirm pacquet's prune behavior can be disabled equivalently.
