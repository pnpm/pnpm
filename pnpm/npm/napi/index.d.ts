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
   * Install from the lockfile without gating on the `package.json` ↔
   * `pnpm-lock.yaml` freshness check, so an in-memory manifest that disagrees
   * with the lockfile does not block the install.
   */
  ignorePackageManifest?: boolean
  /** pnpm home directory. Accepted for compatibility; unused for project installs. */
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
}

/** pnpm's `peerDependencyRules`. */
export interface PeerDependencyRules {
  ignoreMissing?: string[]
  allowAny?: string[]
  allowedVersions?: Record<string, string>
}

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
 */
export function install(
  options: InstallOptions,
  onLog?: LogListener,
  readPackageHook?: ReadPackageHook,
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

/** Version of the underlying Rust engine (pacquet). */
export function engineVersion(): string
