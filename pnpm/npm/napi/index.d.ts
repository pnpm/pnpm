/**
 * Node API bindings for the pnpm v12 Rust engine (pacquet).
 *
 * Shapes intentionally mirror the pnpm v11 TypeScript programmatic API
 * (`@pnpm/installing.deps-installer`, `@pnpm/installing.client`) so that
 * consumers migrating from the TS engine keep their call sites stable.
 */

export interface PackageManifest {
  name?: string
  version?: string
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
  peerDependencies?: Record<string, string>
  peerDependenciesMeta?: Record<string, { optional?: boolean }>
  dependenciesMeta?: Record<string, { injected?: boolean }>
  bundledDependencies?: string[] | boolean
  scripts?: Record<string, string>
  bin?: string | Record<string, string>
  engines?: Record<string, string>
  os?: string[]
  cpu?: string[]
  libc?: string[]
  [key: string]: unknown
}

export interface NodeApiProject {
  /** Absolute path of the importer directory. */
  rootDir: string
  /** In-memory manifest; the engine never reads package.json from disk for listed projects. */
  manifest: PackageManifest
  /**
   * Manifest used when this project is resolved as a *dependency* of another
   * importer (an injected workspace instance) instead of `manifest`. Lets a
   * host pre-transform its importer manifests (e.g. strip workspace-sibling
   * deps it links itself) while dependency instances keep the raw graph —
   * without a `readPackage` hook round trip. Omit when both views are the
   * same.
   */
  dependencyManifest?: PackageManifest
}

export interface ProxyConfig {
  httpProxy?: string
  httpsProxy?: string
  noProxy?: string | boolean
}

export interface NetworkConfig {
  ca?: string | string[]
  cert?: string | string[]
  key?: string
  localAddress?: string
  strictSsl?: boolean
  /**
   * Maximum number of concurrent connections (sockets) to a single registry
   * origin — pnpm's `maxSockets`. Bounds each `scheme://host[:port]` origin
   * independently; the global `networkConcurrency` remains the outer cap.
   */
  maxSockets?: number
  networkConcurrency?: number
  fetchRetries?: number
  fetchRetryFactor?: number
  fetchRetryMintimeout?: number
  fetchRetryMaxtimeout?: number
  fetchTimeout?: number
  fetchWarnTimeoutMs?: number
  fetchMinSpeedKiBps?: number
  userAgent?: string
}

/**
 * A synchronous `readPackage` hook applied to resolved dependency manifests.
 *
 * `resolvedDir` is set when the manifest came from a directory resolution (an
 * injected workspace project or a `file:` dependency); it is the directory
 * recorded in the lockfile, relative to the lockfile root. Hosts use it to
 * recognize a workspace project's dependency instance and substitute the
 * project's raw manifest.
 */
export type ReadPackageHook = (manifest: PackageManifest, resolvedDir?: string) => PackageManifest

/**
 * Receives engine log events. The event stream is wire-compatible with
 * `@pnpm/core-loggers` / the bunyan-shaped objects consumed by
 * `@pnpm/logger`'s streamParser and `@pnpm/cli.default-reporter`.
 */
export type LogListener = (event: Record<string, unknown>) => void

export interface SharedEngineOptions {
  /** Registry routes: `{ default: url, '@scope': url, ... }` */
  registries?: Record<string, string>
  /**
   * Pre-computed `Authorization` header values keyed by nerf-darted registry
   * URI (`//host/path/`), plus `''` for the default registry — e.g.
   * `{ '': 'Bearer abc', '//npm.example.com/': 'Basic <base64(user:pass)>' }`. The
   * host resolves these from its `authConfig`; the engine applies them as-is.
   * The `''` entry is pinned to the `registry` / `registries.default` passed
   * alongside it (npmjs when neither is given), so a `registry=` in the
   * project's own `.npmrc` cannot redirect that credential to another host.
   */
  authHeaderByUri?: Record<string, string>
  proxyConfig?: ProxyConfig
  networkConfig?: NetworkConfig
  cacheDir?: string
}

/** Manifest fields to add to a matching package. */
export interface PackageExtension {
  dependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
  peerDependencies?: Record<string, string>
  peerDependenciesMeta?: Record<string, { optional?: boolean }>
}

export interface InstallOptions extends SharedEngineOptions {
  /** Lockfile / workspace root directory. */
  dir: string
  projects: NodeApiProject[]
  storeDir?: string
  /** Slow metadata-request warning threshold in milliseconds. Overrides `networkConfig`. */
  fetchWarnTimeoutMs?: number
  /** Minimum average tarball download speed in KiB/s. Overrides `networkConfig`. */
  fetchMinSpeedKiBps?: number
  nodeLinker?: 'hoisted' | 'isolated'
  /**
   * pnpm's `linkWorkspacePackages`. When `true`/`'deep'`, a bare-semver
   * dependency (including an auto-installed peer) may resolve to a workspace
   * package by name; `false` (the default) matches only `workspace:` ranges.
   */
  linkWorkspacePackages?: boolean | 'deep'
  hoistPattern?: string[]
  publicHoistPattern?: string[]
  /** Packages linked from outside the workspace; excluded from hoisting/pruning. */
  externalDependencies?: string[]
  overrides?: Record<string, string>
  allowBuilds?: Record<string, boolean>
  dangerouslyAllowAllBuilds?: boolean
  autoInstallPeers?: boolean
  excludeLinksFromLockfile?: boolean
  lockfileOnly?: boolean
  frozenLockfile?: boolean
  preferFrozenLockfile?: boolean
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone'
  preferOffline?: boolean
  offline?: boolean
  virtualStoreDirMaxLength?: number
  /** Whether to use the shared global virtual store for dependency slots. */
  enableGlobalVirtualStore?: boolean
  /** Overrides the global virtual store directory. */
  globalVirtualStoreDir?: string
  /** Manifest fields to add to packages selected by name or version range. */
  packageExtensions?: Record<string, PackageExtension>
  /** Patch paths keyed by package selector. Relative paths resolve from `dir`. */
  patchedDependencies?: Record<string, string>
  /**
   * Warn instead of failing with `ERR_PNPM_UNUSED_PATCH` when a
   * `patchedDependencies` entry matches no installed package. Lets an embedder
   * ship a patch keyed to a version range that only some workspaces resolve.
   */
  allowUnusedPatches?: boolean
  peersSuffixMaxLength?: number
  dedupePeerDependents?: boolean
  /**
   * Render every resolved-peer slot in depPath suffixes as `name@version`
   * instead of the peer's own depPath (the `dedupePeers` setting). Must match
   * the value the existing lockfile was generated with, or the install
   * re-resolves from scratch.
   */
  dedupePeers?: boolean
  dedupeDirectDeps?: boolean
  dedupeInjectedDeps?: boolean
  resolvePeersFromWorkspaceRoot?: boolean
  injectWorkspacePackages?: boolean
  hoistWorkspacePackages?: boolean
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  includeOptionalDeps?: boolean
  ignoreScripts?: boolean
  /**
   * Trust lockfile resolutions without verifying them against current registry
   * metadata.
   */
  trustLockfile?: boolean
  /**
   * Re-resolve the whole dependency graph to the highest in-range version
   * (pnpm's `update: true` / `depth: Infinity`). The binding takes no package
   * selectors, so an update always targets every dependency.
   */
  update?: boolean
  /**
   * pnpm's `depth`. Accepted for API compatibility; it only toggles pnpm's
   * direct-vs-any-depth selector matching, which has no effect without package
   * selectors, so it does not change the whole-graph `update` behavior.
   */
  depth?: number
  /**
   * Fail the install with `ERR_PNPM_UNSUPPORTED_ENGINE` when a dependency's
   * `engines` / platform constraint the host does not satisfy is required
   * (rather than warning). Defaults to `false`.
   */
  engineStrict?: boolean
  /**
   * Node.js version used as the `engines.node` target for the engine check.
   * Defaults to the version auto-detected from the `node` binary.
   */
  nodeVersion?: string
  /**
   * `false` installs without creating a `node_modules` directory: the graph
   * resolves and the lockfile is written, but nothing is materialized.
   */
  enableModulesDir?: boolean
  /**
   * Install from the lockfile alone, ignoring the project manifests —
   * pnpm's `pnpm fetch` semantics. The resolution step and the
   * `package.json` ↔ `pnpm-lock.yaml` freshness check are skipped, every
   * importer the lockfile records is imported into the virtual store, and
   * no post-import linking is performed: no importer symlinks, no `.bin`
   * entries, no hoisting, no project lifecycle scripts.
   */
  ignorePackageManifest?: boolean
  /**
   * The pnpm home directory the default store location is resolved under
   * when no `storeDir` is configured (`<pnpmHomeDir>/store`, with pnpm's
   * same-volume fallback). An explicit `storeDir` — passed here or set by
   * a config source — wins.
   */
  pnpmHomeDir?: string
  /**
   * Fail with `ERR_PNPM_IGNORED_BUILDS` when a dependency build script is
   * blocked. Defaults to `false`: the blocked packages are reported in
   * `InstallResult.depsRequiringBuild` instead.
   */
  strictDepBuilds?: boolean
  /**
   * Report in `InstallResult.depsRequiringBuild` the dep path of every
   * package whose files carry install scripts, regardless of the
   * allow-build policy. The list is computed only when a fresh resolve
   * materializes `node_modules`; an install served from the
   * frozen-lockfile path (or `lockfileOnly`) leaves the field undefined
   * so the embedder keeps its previously recorded list.
   */
  returnListOfDepsRequiringBuild?: boolean
  /** Customizations for how peer-dependency mismatches are treated. */
  peerDependencyRules?: PeerDependencyRules
  /**
   * Render pnpm's own terminal output for this call. Omitted, the call
   * prints nothing and the host renders the `onLog` stream itself (or not
   * at all).
   */
  reporter?: ReporterOptions
}

/** pnpm's `peerDependencyRules`. */
export interface PeerDependencyRules {
  ignoreMissing?: string[]
  allowAny?: string[]
  allowedVersions?: Record<string, string>
}


/**
 * pnpm's own terminal output, rendered by the engine.
 *
 * Without this the host gets only the `onLog` event stream and has to
 * render it itself — in practice by keeping `@pnpm/logger` and
 * `@pnpm/cli.default-reporter` and feeding the events into them, a
 * coupling between one pnpm line's reporter and another's event stream
 * that the host then has to maintain. Set `reporter` and the engine
 * renders with the reporter `pnpm install` itself uses.
 *
 * Every field maps onto the option of the same name in
 * `@pnpm/cli.default-reporter`'s `reportingOptions`.
 */
export interface ReporterOptions {
  /**
   * Print each update on its own line instead of redrawing the frame in
   * place. Defaults to `true` whenever the output is not a terminal.
   */
  appendOnly?: boolean
  /**
   * Milliseconds between progress redraws. Defaults to 1000 in
   * append-only mode and 200 otherwise.
   */
  throttleProgress?: number
  /** Leave the materialized-package count out of the progress line. */
  hideAddedPkgsProgress?: boolean
  /** Leave the workspace-project prefix out of progress lines. */
  hideProgressPrefix?: boolean
  /**
   * Keep dependency build-script output in its collapsed block instead of
   * streaming every line.
   */
  hideLifecycleOutput?: boolean
  /**
   * Replaces the `Run "pnpm approve-builds"…` line under the list of
   * packages whose build scripts were blocked, for a host whose users
   * approve builds through its own configuration.
   */
  ignoredBuildsInstructionText?: string
  /**
   * Package-name patterns whose *linked* entries are left out of the
   * packages-diff summary — an entry is linked when it was symlinked in
   * rather than materialized from the store. A host that links its own
   * runtime into every project silences that noise without silencing the
   * same packages when they are really installed. The Rust counterpart of
   * the TypeScript reporter's `filterPkgsDiff` callback, which cannot
   * cross the addon boundary.
   */
  hideLinkedPkgsDiff?: string[]
  /** Verbosity ceiling. Defaults to `'info'`. */
  logLevel?: 'error' | 'warn' | 'info' | 'debug'
  /**
   * Width to wrap at, at least one column. Defaults to the output stream's
   * width when it is a terminal, else 80. Pass it explicitly alongside
   * `onOutput`: the engine cannot see where those chunks end up.
   */
  width?: number
  /**
   * Whether to emit ANSI color. Defaults to "the output stream is a
   * terminal and `NO_COLOR` is unset"; with `onOutput`, to `false`.
   */
  color?: boolean
  /** Render on stderr rather than stdout. Ignored when `onOutput` is given. */
  useStderr?: boolean
  /** Directory paths are rendered relative to. Defaults to `dir`. */
  cwd?: string
}

/**
 * Receives each rendered output chunk instead of the engine writing it to
 * a file descriptor. For a host that has redirected its own output at the
 * JavaScript level — a monkey-patched `process.stdout.write`, a stream
 * that forwards to a remote terminal — where a write from Rust would
 * bypass the redirection. Chunks arrive in order and already carry their
 * newlines and cursor-control sequences; write them verbatim.
 */
export type OutputListener = (chunk: string) => void

export interface InstallResult {
  stats: {
    added: number
    removed: number
    linkedToRoot: number
  }
  /**
   * With `returnListOfDepsRequiringBuild`: the dep path of every package
   * whose files carry install scripts, whether or not the scripts were
   * allowed to run; undefined when the install did not compute the list
   * (see the option). Without it: the dep paths whose build scripts were
   * skipped and require approval to run.
   */
  depsRequiringBuild?: string[]
  /** The resolved content-addressable store directory used by this install. */
  storeDir: string
}

/**
 * @param onLog receives wire-compatible pnpm log events.
 * @param readPackageHook a **synchronous** `(manifest, resolvedDir?) => manifest`
 *   transform applied to every resolved dependency manifest during resolution
 *   (the `readPackage` hook). Must return the manifest object, not a promise.
 * @param onOutput receives the rendered output of `options.reporter`
 *   instead of the engine writing it to stdout/stderr.
 */
export function install(
  options: InstallOptions,
  onLog?: LogListener,
  readPackageHook?: ReadPackageHook,
  onOutput?: OutputListener,
): Promise<InstallResult>

/**
 * Rebuild dependency build scripts against the already-materialized
 * `node_modules` (frozen path). Takes the same options shape as `install`.
 * @param selectedNames restrict the rebuild to these package names / build
 *   keys; omit (or pass an empty array) to rebuild every build-needing package.
 */
export function rebuild(
  options: InstallOptions,
  onLog?: LogListener,
  selectedNames?: string[],
  onOutput?: OutputListener,
): Promise<void>

export interface PeerIssuesOptions extends SharedEngineOptions {
  dir: string
  projects: NodeApiProject[]
  storeDir?: string
  overrides?: Record<string, string>
  peersSuffixMaxLength?: number
  virtualStoreDirMaxLength?: number
  /**
   * Defaults to `false` for this query (unlike `install`): with peers
   * auto-installed the resolver satisfies them itself and nothing is
   * reported, defeating the report's purpose.
   */
  autoInstallPeers?: boolean
}

export interface PeerDependencyIssues {
  missing: Record<string, Array<{ parents: Array<{ name: string; version: string }>; optional: boolean; wantedRange: string }>>
  bad: Record<string, Array<{ parents: Array<{ name: string; version: string }>; foundVersion: string; resolvedFrom: Array<{ name: string; version: string }>; optional: boolean; wantedRange: string }>>
  conflicts: string[]
  intersections: Record<string, string>
}

export type PeerDependencyIssuesByProjects = Record<string, PeerDependencyIssues>

export function getPeerDependencyIssues(options: PeerIssuesOptions): Promise<PeerDependencyIssuesByProjects>

export interface WantedDependency {
  alias?: string
  bareSpecifier?: string
}

export interface ResolveOptions extends SharedEngineOptions {
  /** Project/lockfile dir used to resolve `link:`/`file:` and workspace specs. */
  dir: string
  /** Return the full packument-derived manifest instead of the abbreviated one. */
  fullMetadata?: boolean
}

export interface ResolveResult {
  id: string
  manifest?: PackageManifest
  resolvedVia: string
  normalizedBareSpecifier?: string
  latest?: string
  resolution?: Record<string, unknown>
}

export function resolveDependency(wanted: WantedDependency, options: ResolveOptions): Promise<ResolveResult>

export interface PackOptions {
  dir: string
  workspaceDir?: string
  /** Destination directory for the tarball (defaults to `dir`). */
  packDestination?: string
  /** Exact output path/filename for the tarball. */
  out?: string
  ignoreScripts?: boolean
  packGzipLevel?: number
  embedReadme?: boolean
  dryRun?: boolean
  extraBinPaths?: string[]
  extraEnv?: Record<string, string>
}

export interface PackResult {
  publishedManifest: PackageManifest
  contents: string[]
  tarballPath: string
  unpackedSize: number
}

export function pack(options: PackOptions, onLog?: LogListener): Promise<PackResult>

export interface ParsedBareSpecifier {
  alias?: string
  bareSpecifier?: string
  name?: string
  fetchSpec?: string
  normalizedBareSpecifier?: string
  type?: string
}

/** Parses/validates a dependency specifier. Returns null for unparsable input. */
export function parseBareSpecifier(spec: string, alias?: string): ParsedBareSpecifier | null

export interface ReadConfigOptions {
  /**
   * Directory whose config cascade to resolve — its `.npmrc`, the enclosing
   * workspace's `pnpm-workspace.yaml` and `.npmrc`, the user and global
   * config files, and `npm_config_*` environment variables.
   */
  dir: string
}

/** One configured registry: the `default` entry plus one per `@scope`. */
export interface ResolvedRegistry {
  /** `"default"` or the package scope (`"@teambit"`). */
  name: string
  url: string
  /**
   * Ready-to-send `Authorization` header for this registry, when the config
   * carries a static credential for it. `tokenHelper` credentials are not
   * executed here and yield no header.
   */
  authHeader?: string
}

export interface ResolvedConfig {
  registries: ResolvedRegistry[]
  /**
   * Static `Authorization` headers keyed by nerf-darted registry URI
   * (`//host[:port]/path/`) — the shape `install`'s `authHeaderByUri`
   * accepts.
   */
  authHeaderByUri: Record<string, string>
  httpProxy?: string
  httpsProxy?: string
  /** `true` (bypass every proxy) or a comma-separated host list. */
  noProxy?: boolean | string
  /** PEM-encoded CA certificates (`ca` / `cafile` already merged). */
  ca?: string[]
  cert?: string
  key?: string
  strictSsl?: boolean
  storeDir: string
  cacheDir: string
  virtualStoreDirMaxLength: number
  /** Whether the resolved configuration uses the global virtual store. */
  enableGlobalVirtualStore: boolean
  /** Shared virtual-store root. */
  globalVirtualStoreDir: string
  /** Project-local virtual-store directory. */
  virtualStoreDir: string
  /** Virtual-store directory used by this configuration and recorded in `.modules.yaml`. */
  effectiveVirtualStoreDir: string
  networkConcurrency: number
  maxSockets?: number
  fetchRetries: number
  fetchRetryFactor: number
  fetchRetryMintimeout: number
  fetchRetryMaxtimeout: number
  fetchTimeout: number
  fetchWarnTimeoutMs: number
  fetchMinSpeedKiBps: number
  /**
   * The explicitly configured user agent, when the cascade set one. The
   * engine's own computed default is omitted — an embedder that passes
   * nothing back to `install` gets that same default.
   */
  userAgent?: string
  engineStrict: boolean
  nodeVersion?: string
  /** `"auto"` / `"hardlink"` / `"copy"` / `"clone"` / `"clone-or-copy"`. */
  packageImportMethod: string
  hoistPattern?: string[]
  publicHoistPattern?: string[]
  /**
   * The legacy `shamefullyHoist` flag; `publicHoistPattern` already
   * reflects it, exposed for embedders that branch on the flag itself.
   */
  shamefullyHoist: boolean
  /**
   * The engine's pnpm home directory (`PNPM_HOME` or the platform default) —
   * not a per-project setting. Absent when no home directory is resolvable.
   */
  pnpmHomeDir?: string
  /**
   * The camelCase names of settings the cascade set explicitly
   * (`pnpm-workspace.yaml`, the global config, `pnpm_config_*` env vars).
   * Every other projected value is an engine default; an embedder that
   * layers this config over its own must forward only the explicit ones.
   */
  explicitSettings: string[]
}

/**
 * Resolve the configuration the engine's own installs use — registries,
 * credentials, proxy, TLS, and network settings from the `.npmrc` cascade —
 * so the embedder needs no JavaScript config reader.
 */
export function readConfig(options: ReadConfigOptions): ResolvedConfig

/**
 * Inputs for {@link getDependents} — the engine side of `pnpm why`.
 *
 * The reverse tree is pure lockfile analysis, so a host that asks the
 * engine for it needs neither `@pnpm/deps.inspection.tree-builder` and
 * `@pnpm/deps.inspection.list` nor the `@pnpm/lockfile.fs` /
 * `@pnpm/installing.modules-yaml` readers that feed them.
 */
export interface DependentsOptions {
  /** Lockfile / workspace root directory. */
  dir: string
  /** Package selectors to search for: a name, or `name@range`. */
  packages: string[]
  /**
   * Importer directories to walk from. Absolute, or relative to `dir`.
   * Omitted means every importer the lockfile records.
   */
  projectDirs?: string[]
  /**
   * Importer-id patterns to skip when `projectDirs` is omitted, in pnpm's
   * `hoistPattern` glob syntax (`*` is the only wildcard). Lets a host keep
   * its own generated importers out of the answer without reading the
   * lockfile itself to enumerate the rest.
   */
  excludeProjectPatterns?: string[]
  /** `node_modules` directory. Defaults to `<dir>/node_modules`. */
  modulesDir?: string
  /** Follow `dependencies` edges. Defaults to `true`. */
  includeDependencies?: boolean
  /** Follow `devDependencies` edges. Defaults to `true`. */
  includeDevDependencies?: boolean
  /** Follow `optionalDependencies` edges. Defaults to `true`. */
  includeOptionalDependencies?: boolean
  /** Registry routes, used to reconstruct tarball URLs. */
  registries?: Record<string, string>
  /** Fallback when `.modules.yaml` records no value. */
  virtualStoreDirMaxLength?: number
  /**
   * `package.json` fields to project onto every package node as
   * `manifest`. This is what the TypeScript tree-builder's `nameFormatter`
   * callback is for: the walk is synchronous Rust and cannot call back
   * into JavaScript, so a host that renames nodes after a manifest field
   * asks for that field here, writes `displayName` on the returned trees,
   * and passes them to {@link renderDependents}. Nodes whose manifest is
   * unreadable — and every workspace-project node — carry none.
   */
  manifestFields?: string[]
}

/** One entry of a {@link DependentsTree}'s reverse tree. */
export interface DependentNode {
  name: string
  /** Rendered in place of `name`, when set. */
  displayName?: string
  version: string
  /** The node was reached again on its own path; the walk stopped there. */
  circular?: boolean
  /** Short hash distinguishing peer-dependency variants of a `name@version`. */
  peersSuffixHash?: string
  /** The node is expanded elsewhere in the tree and shown here as a leaf. */
  deduped?: boolean
  /** For a workspace-project leaf: which manifest field declares the edge. */
  depField?: 'dependencies' | 'devDependencies' | 'optionalDependencies'
  dependents?: DependentNode[]
  /** The `manifestFields` projection of this node's `package.json`. */
  manifest?: Record<string, unknown>
}

/** One matched package and everything that depends on it. */
export interface DependentsTree {
  name: string
  /** Rendered in place of `name`, when set. */
  displayName?: string
  version: string
  /** Resolved filesystem path of the package. */
  path?: string
  peersSuffixHash?: string
  dependents: DependentNode[]
  /** Message returned by a `--find-by` finder, when one matched. */
  searchMessage?: string
  /** See {@link DependentNode.manifest}. */
  manifest?: Record<string, unknown>
}

/**
 * Every package matching `packages`, each with the reverse tree of what
 * depends on it. An empty array when the directory has no lockfile: an
 * un-installed workspace has no dependents to report, which is an answer
 * rather than an error.
 */
export function getDependents(options: DependentsOptions): Promise<DependentsTree[]>

export interface RenderDependentsOptions {
  /** Defaults to `'tree'`. */
  format?: 'tree' | 'parseable' | 'json'
  /** Max display depth. Omitted renders the whole tree. */
  depth?: number
  /** Include description / repository / homepage / path for each root. */
  long?: boolean
}

/**
 * Render trees from {@link getDependents} — after any `displayName` the
 * caller wrote onto them — the way `pnpm why` renders its own.
 */
export function renderDependents(
  trees: DependentsTree[],
  options?: RenderDependentsOptions,
): string

/**
 * A `pnpm-lock.yaml` as JSON — the file's own shape, which is
 * `LockfileFile` in `@pnpm/lockfile.types` terms: each importer dependency
 * is an `{ specifier, version }` pair, and `packages` (metadata) and
 * `snapshots` (edges) are separate maps. There is no in-memory-only
 * variant to convert to or from.
 *
 * Top-level keys pnpm does not define are preserved, so a host that
 * records its own state beside the lockfile can read it, edit its own
 * block, and write the file back without losing anything else.
 *
 * The lockfile functions are generic over this so a host that already has
 * a precise type for the format — `LockfileFile` from
 * `@pnpm/lockfile.types`, or its own extension of it — can name it rather
 * than casting: `readLockfile<MyLockfile>({ dir })`.
 */
export type LockfileFile = Record<string, unknown>

export interface ReadLockfileOptions {
  /** Lockfile / workspace root directory. */
  dir: string
  /**
   * `'wanted'` (the default) reads `<dir>/pnpm-lock.yaml`, what the
   * workspace asks for. `'current'` reads
   * `<modulesDir>/.pnpm/lock.yaml`, what the last install actually
   * materialized.
   */
  kind?: 'wanted' | 'current'
  /**
   * `node_modules` directory, which the current lockfile lives under.
   * Absolute, or relative to `dir`. Defaults to `<dir>/node_modules`.
   */
  modulesDir?: string
}

export interface WriteLockfileOptions<Lockfile = LockfileFile> {
  /** Lockfile / workspace root directory. */
  dir: string
  /** The lockfile to write, in the shape {@link readLockfile} returns. */
  lockfile: Lockfile
  /** See {@link ReadLockfileOptions.kind}. */
  kind?: 'wanted' | 'current'
  /** See {@link ReadLockfileOptions.modulesDir}. */
  modulesDir?: string
}

/** `null` when the lockfile is absent or empty. */
export function readLockfile<Lockfile = LockfileFile>(
  options: ReadLockfileOptions,
): Promise<Lockfile | null>

/** Write the lockfile, formatted exactly as an install writes it. */
export function writeLockfile<Lockfile = LockfileFile>(
  options: WriteLockfileOptions<Lockfile>,
): Promise<void>

export interface FilterLockfileOptions {
  /** Whether the listed importers keep their `dependencies`. Default `true`. */
  includeDependencies?: boolean
  /** Whether they keep their `devDependencies`. Default `true`. */
  includeDevDependencies?: boolean
  /** Whether they keep their `optionalDependencies`. Default `true`. */
  includeOptionalDependencies?: boolean
  /**
   * Dep paths to treat as already visited — the optional dependencies this
   * platform did not install. Neither they nor anything reachable only
   * through them is kept.
   */
  skipped?: string[]
  /**
   * Whether a dependency reference with no `snapshots` entry fails with
   * `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY`. Defaults to `false`, which
   * drops the reference and keeps walking — what a caller inspecting a
   * possibly-stale lockfile wants.
   */
  failOnMissingDependencies?: boolean
}

/**
 * The lockfile narrowed to what `importerIds` reaches: those importers keep
 * only the dependency groups asked for, and `packages` / `snapshots` are
 * pruned to the transitive closure of what they still depend on. Every
 * other importer entry is carried through untouched — the filter narrows
 * the package graph, not the workspace.
 *
 * Synchronous: a transform over data the caller already holds.
 */
export function filterLockfileByImporters<Lockfile = LockfileFile>(
  lockfile: Lockfile,
  importerIds: string[],
  options?: FilterLockfileOptions,
): Lockfile

/**
 * The `.modules.yaml` state of an installed `node_modules`, or `null` when
 * the directory has none.
 */
export function readModulesManifest(modulesDir: string): Promise<Record<string, unknown> | null>

/** Version of the underlying Rust engine (pacquet). */
export function engineVersion(): string
