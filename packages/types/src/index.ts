import '@total-typescript/ts-reset'

import type { IncomingMessage } from 'http'

import semver from 'semver'
import type ssri from 'ssri'
import pLimit from 'p-limit'
import type { IntegrityLike } from 'ssri'
import type { AgentOptions } from '@pnpm/network.agent'

import {
  isRedirect,
  type Request,
  type Response,
  type HeadersInit,
  type RequestInit as NodeRequestInit,
} from 'node-fetch'
import type { JsonObject } from 'type-fest'
import type { SafePromiseDefer } from 'safe-promise-defer'

export {
  isRedirect,
  type Request,
  type Response,
  type HeadersInit,
  type AgentOptions,
}

// NOTE: The order in this array is important.
export const DEPENDENCIES_FIELDS = [
  'optionalDependencies',
  'dependencies',
  'devDependencies',
] as const satisfies DependenciesField[]

export const DEPENDENCIES_OR_PEER_FIELDS = [
  ...DEPENDENCIES_FIELDS,
  'peerDependencies',
] as const satisfies DependenciesOrPeersField[]

export type RegistryPackageSpec = {
  type: 'tag' | 'version' | 'range'
  name: string
  fetchSpec: string
  normalizedPref?: string | undefined
}

export type PickPackageOptions = {
  authHeaderValue?: string | undefined
  publishedBy?: Date | undefined
  preferredVersionSelectors: VersionSelectors | undefined
  pickLowestVersion?: boolean | undefined
  registry: string
  dryRun: boolean
  updateToLatest?: boolean | undefined
}

export type PackageMeta = {
  name: string
  'dist-tags': Record<string, string>
  versions: Record<string, PackageInRegistry>
  time?: PackageMetaTime | undefined
  cachedAt?: number | undefined
}

export type PackageMetaTime = Record<string, string> & {
  unpublished?: {
    time: string
    versions: string[]
  } | undefined
}

export type PackageMetaCache = {
  get: (key: string) => PackageMeta | undefined
  set: (key: string, meta: PackageMeta) => void
  has: (key: string) => boolean
}

export type PackageInRegistry = PackageManifest & {
  dist: {
    integrity?: string | undefined
    shasum: string
    tarball: string
  }
}

export type RefCountedLimiter = {
  count: number
  limit: pLimit.Limit
}

export type PickVersionByVersionRange = (
  meta: PackageMeta,
  versionRange: string,
  preferredVerSels?: VersionSelectors | undefined,
  publishedBy?: Date | undefined
) => string | semver.SemVer | null | undefined

export type RegistryResponse = {
  status: number
  statusText: string
  json: () => Promise<PackageMeta>
}

export type GetDependenciesCacheEntryArgs = {
  readonly parentId: TreeNodeId
  readonly requestedDepth: number
}

export type TraversalResultFullyVisited = {
  readonly dependencies: PackageNode[]

  /**
   * Describes the height of the parent node in the fully enumerated dependency
   * tree. A height of 0 means no entries are present in the dependencies array.
   * A height of 1 means entries in the dependencies array do not have any of
   * their own dependencies.
   */
  readonly height: number
}

export type TraversalResultPartiallyVisited = {
  readonly dependencies: (PackageNode | PackageInfo)[]

  /**
   * Describes how deep the dependencies tree was previously traversed. Since
   * the traversal result was limited by a max depth, there are likely more
   * dependencies present deeper in the tree not shown.
   *
   * A depth of 0 would indicate no entries in the dependencies array. A depth
   * of 1 means entries in the dependencies array do not have any of their own
   * dependencies.
   */
  readonly depth: number
}

export type CacheHit = {
  readonly dependencies: PackageNode[]
  readonly height: number | 'unknown'
  // Circular dependencies are not stored in the cache.
  readonly circular: false
}

export interface PkgData {
  alias: string | undefined
  name: string
  version: string
  path: string
  resolved?: string | undefined
}

export type RenderJsonResultItem = Pick<
  PackageDependencyHierarchy,
  'name' | 'version' | 'path'
> &
  Required<Pick<PackageDependencyHierarchy, 'private'>> & {
    dependencies?: Record<string, PackageJsonListItem>
    devDependencies?: Record<string, PackageJsonListItem>
    optionalDependencies?: Record<string, PackageJsonListItem>
    unsavedDependencies?: Record<string, PackageJsonListItem>
  }

export type PackageJsonListItem = PkgInfo & {
  dependencies?: Record<string, PackageJsonListItem> | undefined
}

export type PkgInfo = Omit<PkgData, 'name'> &
  Pick<ProjectManifest, 'description' | 'license' | 'author' | 'homepage'> & {
    from: string
    repository?: string | undefined
  }

export type GetPkgColor = (node: PackageNode | PackageInfo) => (s: string) => string

export type RenderTreeOptions = {
  alwaysPrintRootPackage: boolean
  depth: number
  long: boolean
  search: boolean
  showExtraneous: boolean
}

export type PackageDependencyHierarchy = DependenciesHierarchy & {
  name?: string | undefined
  version?: string | undefined
  path: string
  private?: boolean | undefined
}

export type AuditVulnerabilityCounts = {
  info: number
  low: number
  moderate: number
  high: number
  critical: number
}

export type AuditResolution = {
  id: number
  path: string
  dev: boolean
  optional: boolean
  bundled: boolean
}

export type AuditAction = {
  action: string
  module: string
  target: string
  isMajor: boolean
  resolves: AuditResolution[]
}

export type AuditAdvisory = {
  findings: [
    {
      version: string
      paths: string[]
      dev: boolean
      optional: boolean
      bundled: boolean
    },
  ]
  id: number
  created: string
  updated: string
  deleted?: boolean | undefined
  title: string
  found_by: {
    name: string
  }
  reported_by: {
    name: string
  }
  module_name: string
  cves: string[]
  vulnerable_versions: string
  patched_versions: string
  overview: string
  recommendation: string
  references: string
  access: string
  severity: string
  cwe: string
  metadata: {
    module_type: string
    exploitability: number
    affected_components: string
  }
  url: string
}

export type AuditMetadata = {
  vulnerabilities: AuditVulnerabilityCounts
  dependencies: number
  devDependencies: number
  optionalDependencies: number
  totalDependencies: number
}

export type AuditReport = {
  actions: AuditAction[]
  advisories: { [id: string]: AuditAdvisory }
  muted: unknown[]
  metadata: AuditMetadata
}

export type AuditActionRecommendation = {
  cmd: string
  isBreaking: boolean
  action: AuditAction
}

export type HttpResponse = {
  body: string
}

export type DownloadFunction = (
  url: string,
  opts: {
    getAuthHeaderByURI: (registry: string) => string | undefined
    cafs: Cafs
    readManifest?: boolean | undefined
    registry?: string | undefined
    onStart?: ((totalSize: number | null, attempt: number) => void) | undefined
    onProgress?: ((downloaded: number) => void) | undefined
    integrity?: string | undefined
    filesIndexFile: string
  } & Pick<FetchOptions, 'pkg'>
) => Promise<FetchResult>

export type NpmRegistryClient = {
  get: (
    url: string,
    getOpts: object,
    cb: (err: Error, data: object, raw: object, res: HttpResponse) => void
  ) => void
  fetch: (
    url: string,
    opts: { auth?: object | undefined },
    cb: (err: Error, res: IncomingMessage) => void
  ) => void
}

export type NvmNodeCommandOptions = Pick<
  Config,
  | 'bin'
  | 'global'
  | 'fetchRetries'
  | 'fetchRetryFactor'
  | 'fetchRetryMaxtimeout'
  | 'fetchRetryMintimeout'
  | 'fetchTimeout'
  | 'userAgent'
  | 'ca'
  | 'cert'
  | 'httpProxy'
  | 'httpsProxy'
  | 'key'
  | 'localAddress'
  | 'noProxy'
  | 'rawConfig'
  | 'strictSsl'
  | 'storeDir'
  | 'useNodeVersion'
  | 'pnpmHomeDir'
> &
  Partial<Pick<Config, 'configDir' | 'cliOptions'>> & {
    remote?: boolean | undefined
  }

export type NodeVersion = {
  version: string
  lts: false | string
}

export type RetryTimeoutOptions = {
  factor: number;
  maxTimeout: number;
  minTimeout: number;
  randomize: boolean;
  retries: number;
}

export type FetchNodeOptions = {
  cafsDir: string
  fetchTimeout?: number | undefined
  nodeMirrorBaseUrl?: string | undefined
  retry?: RetryTimeoutOptions | undefined
}

export type FetchFromRegistry = (
  url: string,
  opts?: {
    authHeaderValue?: string | undefined
    compress?: boolean | undefined
    retry?: RetryTimeoutOptions | undefined
    timeout?: number | undefined
  } | undefined
) => Promise<Response>

export type GetAuthHeader = (uri: string) => string | undefined

export type PackageDiff = {
  added: boolean
  from?: string | undefined
  name: string
  realName?: string | undefined
  version?: string | undefined
  deprecated?: boolean | undefined
  latest?: string | undefined
}

export type ConfigCommandOptions = Pick<
  Config,
  'configDir' | 'cliOptions' | 'dir' | 'global' | 'npmPath' | 'rawConfig'
> & {
  json?: boolean
  location?: 'global' | 'project'
}

export type Engine = { node?: string | undefined; npm?: string | undefined; pnpm?: string | undefined; }

export type WantedEngine = Partial<Engine>

export type Platform = {
  cpu: string | string[]
  os: string | string[]
  libc: string | string[]
}

export type WantedPlatform = Partial<Platform>

export type LicenseNode = {
  name?: string | undefined
  version?: string | undefined
  license: string
  licenseContents?: string | undefined
  dir: string
  author?: string | undefined
  homepage?: string | undefined
  description?: string | undefined
  repository?: string | undefined
  integrity?: string | undefined
  requires?: Record<string, string> | undefined
  dependencies?: { [name: string]: LicenseNode } | undefined
  dev: boolean
}

export type LicenseNodeTree = Omit<
  LicenseNode,
  'dir' | 'license' | 'licenseContents' | 'author' | 'homepages' | 'repository'
>

export type LicenseExtractOptions = {
  storeDir: string
  virtualStoreDir: string
  modulesDir?: string | undefined
  dir: string
  registries: Registries
  supportedArchitectures?: SupportedArchitectures | undefined
}

export type LicensePackage = {
  belongsTo: DependenciesField
  version: string
  name: string
  license: string
  licenseContents?: string | undefined
  author?: string | undefined
  homepage?: string | undefined
  description?: string | undefined
  repository?: string | undefined
  path?: string | undefined
}

export type VersionOverride = {
  parentPkg?: {
    name: string
    pref?: string | undefined
  } | undefined
  targetPkg: {
    name: string
    pref?: string | undefined
  }
  newPref: string
}

export type GetTreeNodeChildIdOpts = {
  readonly parentId: TreeNodeId
  readonly dep: {
    readonly alias: string
    readonly ref: string
  }
  readonly lockfileDir: string
  readonly importers: Record<string, ProjectSnapshot>
}

export type LocalPackageSpec = {
  dependencyPath: string
  fetchSpec: string
  id: string
  type: 'directory' | 'file'
  normalizedPref: string
}

export type WantedLocalDependency = {
  pref: string
  injected?: boolean | undefined
}

export type GetTreeOpts = {
  maxDepth: number
  rewriteLinkVersionDir: string
  includeOptionalDependencies: boolean
  lockfileDir: string
  onlyProjects?: boolean | undefined
  search?: SearchFunction | undefined
  skipped: Set<string>
  registries: Registries
  importers: Record<string, ProjectSnapshot>
  currentPackages: PackageSnapshots
  wantedPackages: PackageSnapshots
  virtualStoreDir?: string | undefined
}

export type DependencyInfo = {
  dependencies: (PackageNode | PackageInfo)[]

  circular?: true | undefined

  /**
   * The number of edges along the longest path, including the parent node.
   *
   *   - `"unknown"` if traversal was limited by a max depth option, therefore
   *      making the true height of a package undetermined.
   *   - `0` if the dependencies array is empty.
   *   - `1` if the dependencies array has at least 1 element and no child
   *     dependencies.
   */
  height: number | 'unknown'
}

export type DependenciesHierarchy = {
  dependencies?: (PackageNode | PackageInfo)[] | undefined
  devDependencies?: (PackageNode | PackageInfo)[] | undefined
  optionalDependencies?: (PackageNode | PackageInfo)[] | undefined
  unsavedDependencies?: (PackageNode | PackageInfo)[] | undefined
}

export type GetPkgInfoOpts = {
  readonly alias: string
  readonly ref: string
  readonly currentPackages: PackageSnapshots
  readonly peers?: Set<string> | undefined
  readonly registries: Registries
  readonly skipped: Set<string>
  readonly wantedPackages: PackageSnapshots
  readonly virtualStoreDir?: string | undefined

  /**
   * The base dir if the `ref` argument is a `"link:"` relative path.
   */
  readonly linkedPathBaseDir: string

  /**
   * If the `ref` argument is a `"link:"` relative path, the ref is reused for
   * the version field. (Since the true semver may not be known.)
   *
   * Optionally rewrite this relative path to a base dir before writing it to
   * version.
   */
  readonly rewriteLinkVersionDir?: string | undefined
}

export type PackageInfo = {
  alias: string
  isMissing: boolean
  isPeer: boolean
  isSkipped: boolean
  name: string
  path: string
  version: string
  resolved?: string | undefined
  optional?: boolean | undefined
  dev?: boolean | undefined
  searched?: boolean | undefined
  circular?: boolean | undefined
  dependencies?: (PackageNode | PackageInfo)[] | undefined
}

export type TreeNodeId = TreeNodeIdImporter | TreeNodeIdPackage

/**
 * A project local to the pnpm workspace.
 */
export type TreeNodeIdImporter = {
  readonly type: 'importer'
  readonly importerId: string
}

/**
 * An npm package depended on externally.
 */
export type TreeNodeIdPackage = {
  readonly type: 'package'
  readonly depPath: string
}

export type PackageNode = {
  alias: string
  circular?: boolean | undefined
  dependencies?: PackageNode[] | undefined
  dev?: boolean
  isPeer: boolean
  isSkipped: boolean
  isMissing: boolean
  name: string
  optional?: boolean | undefined
  path: string
  resolved?: string | undefined
  searched?: boolean | undefined
  version: string
}

export type SearchFunction = (pkg: { name: string; version: string }) => boolean

export type VerifyResult = {
  passed: boolean
  manifest?: DependencyManifest | undefined
}

export type PackageFilesIndex = {
  // name and version are nullable for backward compatibility
  // the initial specs of pnpm store v3 did not require these fields.
  // However, it might be possible that some types of dependencies don't
  // have the name/version fields, like the local tarball dependencies.
  name?: string | undefined
  version?: string | undefined

  files: Record<string, PackageFileInfo>
  sideEffects?: Record<string, Record<string, PackageFileInfo>>
}

export type FileInfo = Pick<PackageFileInfo, 'size' | 'checkedAt'> & {
  integrity: string | ssri.IntegrityLike
}

/**
 * Similar to the current Lockfile importers format (lockfile version 5.4 at
 * time of writing), but specifiers are moved to each ResolvedDependencies block
 * instead of being declared on its own dictionary.
 *
 * This is an experiment to reduce one flavor of merge conflicts in lockfiles.
 * For more info: https://github.com/pnpm/pnpm/issues/4725.
 */
export type InlineSpecifiersLockfile = Omit<Lockfile, 'lockfileVersion' | 'importers'> & {
  lockfileVersion: string
  importers: Record<string, InlineSpecifiersProjectSnapshot>
}

/**
 * Similar to the current ProjectSnapshot interface, but omits the "specifiers"
 * field in favor of inlining each specifier next to its version resolution in
 * dependency blocks.
 */
export type InlineSpecifiersProjectSnapshot = {
  dependencies?: InlineSpecifiersResolvedDependencies | undefined
  devDependencies?: InlineSpecifiersResolvedDependencies | undefined
  optionalDependencies?: InlineSpecifiersResolvedDependencies | undefined
  dependenciesMeta?: DependenciesMeta | undefined
}

export type InlineSpecifiersResolvedDependencies = {
  [depName: string]: SpecifierAndResolution
}

export type SpecifierAndResolution = {
  specifier: string
  version: string
}

export type StrictLinkOptions = {
  autoInstallPeers?: boolean | undefined
  binsDir: string
  excludeLinksFromLockfile?: boolean | undefined
  force: boolean
  forceSharedLockfile: boolean
  useLockfile: boolean
  lockfileDir: string
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  pinnedVersion?: 'major' | 'minor' | 'patch' | undefined
  storeController: StoreController
  manifest?: ProjectManifest | undefined
  registries: Registries
  storeDir?: string | undefined
  reporter?: ReporterFunction | undefined
  targetDependenciesField?: DependenciesField | undefined
  dir: string
  preferSymlinkedExecutables?: boolean | undefined

  hoistPattern?: string[] | undefined
  forceHoistPattern?: boolean | undefined

  publicHoistPattern?: string[] | undefined
  forcePublicHoistPattern?: boolean | undefined

  useGitBranchLockfile?: boolean | undefined
  mergeGitBranchLockfiles?: boolean | undefined
}

export type LinkOptions = Partial<StrictLinkOptions> &
  Pick<StrictLinkOptions, 'storeController' | 'manifest'>

export type GetLatestManifestFunction = (
  packageName: string,
  rangeOrTag: string
) => Promise<PackageManifest | null>

export interface OutdatedPackage {
  alias: string
  belongsTo: DependenciesField
  current?: string | undefined // not defined means the package is not installed
  latestManifest?: PackageManifest | undefined
  packageName: string
  wanted: string
  workspace?: string | undefined
}

export type SEMVER_CHANGE = 'breaking' | 'feature' | 'fix' | 'unknown';

export type OutdatedWithVersionDiff = OutdatedPackage & {
  change: SEMVER_CHANGE | null
  diff?: [string[], string[]]
}

export type OutdatedCommandOptions = {
  compatible?: boolean | undefined
  long?: boolean | undefined
  recursive?: boolean | undefined
  format?: 'table' | 'list' | 'json' | undefined
} & Pick<
  Config,
  | 'allProjects'
  | 'ca'
  | 'cacheDir'
  | 'cert'
  | 'dev'
  | 'dir'
  | 'engineStrict'
  | 'fetchRetries'
  | 'fetchRetryFactor'
  | 'fetchRetryMaxtimeout'
  | 'fetchRetryMintimeout'
  | 'fetchTimeout'
  | 'global'
  | 'httpProxy'
  | 'httpsProxy'
  | 'key'
  | 'localAddress'
  | 'lockfileDir'
  | 'networkConcurrency'
  | 'noProxy'
  | 'offline'
  | 'optional'
  | 'production'
  | 'rawConfig'
  | 'registries'
  | 'selectedProjectsGraph'
  | 'strictSsl'
  | 'tag'
  | 'userAgent'
> &
  Partial<Pick<Config, 'userConfig'>>

export interface OutdatedPackageJSONOutput {
  current?: string | undefined
  latest?: string | undefined
  wanted: string
  isDeprecated: boolean
  dependencyType: DependenciesField
  latestManifest?: PackageManifest | undefined
}

export type RecursiveRunOpts = Pick<
  Config,
  | 'enablePrePostScripts'
  | 'unsafePerm'
  | 'rawConfig'
  | 'rootProjectManifest'
  | 'scriptsPrependNodePath'
  | 'scriptShell'
  | 'shellEmulator'
  | 'stream'
> &
  Required<
    Pick<
      Config,
      'allProjects' | 'selectedProjectsGraph' | 'workspaceDir' | 'dir'
    >
  > &
  Partial<
    Pick<
      Config,
      | 'extraBinPaths'
      | 'extraEnv'
      | 'bail'
      | 'reverse'
      | 'sort'
      | 'workspaceConcurrency'
    >
  > & {
    ifPresent?: boolean | undefined
    resumeFrom?: string | undefined
    reportSummary?: boolean | undefined
  }

export type ErrorRelatedSources = {
  additionalInformation?: string | undefined
  relatedIssue?: number | undefined
  relatedPR?: number | undefined
}

export type FetchWithAgentOptions = RequestInit & {
  agentOptions: AgentOptions
}

export type URLLike = {
  href: string
}

export type RequestInfo = string | URLLike | Request

export type RequestInit = NodeRequestInit & {
  retry?: RetryTimeoutOptions | undefined
  timeout?: number | undefined
}

export type HookMessage = {
  from: string
  hook: string
  message: string
  prefix: string
}

export type HookLog = { name: 'pnpm:hook' } & LogBase & HookMessage

export type StatsMessage = {
  prefix: string
} & (
  | {
    added: number
  }
  | {
    removed: number
  }
)

export type StatsLog = { name: 'pnpm:stats' } & LogBase & StatsMessage

export type ContextMessage = {
  currentLockfileExists: boolean
  storeDir: string
  virtualStoreDir: string
}

export type ContextLog = { name: 'pnpm:context' } & LogBase & ContextMessage

export type DeprecationMessage = {
  pkgName: string
  pkgVersion: string
  pkgId: string
  prefix: string
  deprecated: string
  depth: number
}

export type DeprecationLog = { name: 'pnpm:deprecation' } & LogBase &
  DeprecationMessage

export type ExecutionTimeMessage = {
  startedAt: number
  endedAt: number
}

export type ExecutionTimeLog = { name: 'pnpm:execution-time' } & LogBase &
  ExecutionTimeMessage

export type FetchingProgressMessage =
  | {
    attempt: number
    packageId: string
    size: number | null
    status: 'started'
  }
  | {
    downloaded: number
    packageId: string
    status: 'in_progress'
  }

export type FetchingProgressLog = { name: 'pnpm:fetching-progress' } & LogBase &
  FetchingProgressMessage

export type InstallCheckMessage = {
  code: string
  pkgId: string
}

export type InstallCheckLog = { name: 'pnpm:install-check' } & LogBase &
  InstallCheckMessage

// TODO: make depPath optional
export type LifecycleMessage = {
  depPath: string
  stage: string
  wd: string
} & (
  | {
    line: string
    stdio: 'stdout' | 'stderr'
  }
  | {
    exitCode: number
    optional: boolean
  }
  | {
    script: string
    optional: boolean
  }
)

export type LifecycleLog = { name: 'pnpm:lifecycle' } & LogBase &
  LifecycleMessage

export type LinkMessage = {
  target: string
  link: string
}

export type LinkLog = { name: 'pnpm:link' } & LogBase & LinkMessage

export type PackageImportMethodMessage = {
  method: 'clone' | 'hardlink' | 'copy'
}

export type PackageImportMethodLog = {
  name: 'pnpm:package-import-method'
} & LogBase &
  PackageImportMethodMessage

export type PackageManifestMessage = {
  prefix: string
} & (
  | {
    initial?: ProjectManifest | undefined
  }
  | {
    updated: ProjectManifest
  }
)

export type PackageManifestLog = { name: 'pnpm:package-manifest' } & LogBase &
  PackageManifestMessage

export type PeerDependencyIssuesMessage = {
  issuesByProjects: PeerDependencyIssuesByProjects
}

export type PeerDependencyIssuesLog = {
  name: 'pnpm:peer-dependency-issues'
} & LogBase &
  PeerDependencyIssuesMessage

export type ProgressMessage =
  | {
    packageId: string
    requester: string
    status: 'fetched' | 'found_in_store' | 'resolved'
  }
  | {
    status: 'imported'
    method: string
    requester: string
    to: string
  }

export type ProgressLog = { name: 'pnpm:progress' } & LogBase & ProgressMessage

export type RegistryMessage = {
  message: string
}

export type RegistryLog = { name: 'pnpm:registry' } & LogBase & RegistryMessage

export type RequestRetryMessage = {
  attempt: number
  error: Error
  maxRetries: number
  method: string
  timeout: number
  url: string
}

export type RequestRetryLog = { name: 'pnpm:request-retry' } & LogBase &
  RequestRetryMessage

export type DependencyType = 'prod' | 'dev' | 'optional'

export type RootMessage = {
  prefix: string
} & (
  | {
    added: {
      id?: string | undefined
      name: string
      realName: string
      version?: string | undefined
      dependencyType?: DependencyType | undefined
      latest?: string | undefined
      linkedFrom?: string | undefined
    }
  }
  | {
    removed: {
      name: string
      version?: string | undefined
      dependencyType?: DependencyType | undefined
    }
  }
)

export type RootLog = { name: 'pnpm:root' } & LogBase & RootMessage

export type ScopeMessage = {
  selected: number
  total?: number | undefined
  workspacePrefix?: string | undefined
}

export type ScopeLog = { name: 'pnpm:scope' } & LogBase & ScopeMessage

export type StageMessage = {
  prefix: string
  stage:
    | 'resolution_started'
    | 'resolution_done'
    | 'importing_started'
    | 'importing_done'
}

export type StageLog = { name: 'pnpm:stage' } & LogBase & StageMessage

export type SummaryMessage = {
  prefix: string
}

export type SummaryLog = { name: 'pnpm:summary' } & LogBase & SummaryMessage

export type UpdateCheckMessage = {
  currentVersion: string
  latestVersion: string
}

export type UpdateCheckLog = { name: 'pnpm:update-check' } & LogBase &
  UpdateCheckMessage

export type Log =
  | ContextLog
  | DeprecationLog
  | FetchingProgressLog
  | ExecutionTimeLog
  | HookLog
  | InstallCheckLog
  | LifecycleLog
  | LinkLog
  | PackageManifestLog
  | PackageImportMethodLog
  | PeerDependencyIssuesLog
  | ProgressLog
  | RegistryLog
  | RequestRetryLog
  | RootLog
  | ScopeLog
  | SkippedOptionalDependencyLog
  | StageLog
  | StatsLog
  | SummaryLog
  | UpdateCheckLog

export type HookContext = {
  log: (message: string) => void
}

export type CustomFetcherFactoryOptions = {
  defaultFetchers: Fetchers
}

export type CustomFetcherFactory<Fetcher> = (
  opts: CustomFetcherFactoryOptions
) => Fetcher

export type CustomFetchers = {
  localTarball?: CustomFetcherFactory<FetchFunction> | undefined
  remoteTarball?: CustomFetcherFactory<FetchFunction> | undefined
  gitHostedTarball?: CustomFetcherFactory<FetchFunction> | undefined
  directory?: CustomFetcherFactory<DirectoryFetcher> | undefined
  git?: CustomFetcherFactory<GitFetcher> | undefined
}

export type Hook = {

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  readPackage?: ((pkg: any, context: HookContext) => any) | undefined
  preResolution?: PreResolutionHook | undefined
  afterAllResolved?: ((
    lockfile: Lockfile,
    context: HookContext
  ) => Lockfile | Promise<Lockfile>) | undefined
  filterLog?: ((log: Log) => boolean) | undefined
  importPackage?: ImportIndexedPackageAsync | undefined
  fetchers?: CustomFetchers | undefined
}

// eslint-disable-next-line
export type Cook<T extends (...args: any[]) => any> = (
  arg: Parameters<T>[0],
  // eslint-disable-next-line
  ...otherArgs: any[]
) => ReturnType<T>

export type CookedHooks = {
  readPackage?: Array<Cook<Exclude<Hook['readPackage'], undefined>>> | undefined
  preResolution?: Cook<Exclude<Hook['preResolution'], undefined>> | undefined
  afterAllResolved?: Array<Cook<Exclude<Hook['afterAllResolved'], undefined>>> | undefined
  filterLog?: Array<Cook<Exclude<Hook['filterLog'], undefined>>> | undefined
  importPackage?: ImportIndexedPackageAsync | undefined
  fetchers?: CustomFetchers | undefined
}

export type Hooks = CookedHooks

export type UniversalOptions = Pick<
  Config,
  'color' | 'dir' | 'rawConfig' | 'rawLocalConfig'
>

export type Config = {
  allProjects?: Project[] | undefined
  selectedProjectsGraph?: ProjectsGraph | undefined
  allProjectsGraph?: ProjectsGraph | undefined

  allowNew: boolean
  autoInstallPeers?: boolean | undefined
  bail: boolean
  color: 'always' | 'auto' | 'never'
  cliOptions: Record<string, any>, // eslint-disable-line
  useBetaCli: boolean
  excludeLinksFromLockfile: boolean
  extraBinPaths: string[]
  extraEnv: Record<string, string>
  failIfNoMatch: boolean
  filter: string[]
  filterProd: string[]
  rawLocalConfig: Record<string, any>, // eslint-disable-line
  rawConfig: Record<string, any>, // eslint-disable-line
  dryRun?: boolean | undefined // This option might be not supported ever
  global?: boolean | undefined
  dir: string
  bin: string
  ignoreDepScripts?: boolean | undefined
  ignoreScripts?: boolean | undefined
  ignoreCompatibilityDb?: boolean | undefined
  includeWorkspaceRoot?: boolean | undefined
  save?: boolean | undefined
  saveProd?: boolean | undefined
  saveDev?: boolean | undefined
  saveOptional?: boolean | undefined
  savePeer?: boolean | undefined
  saveWorkspaceProtocol?: boolean | 'rolling' | undefined
  lockfileIncludeTarballUrl?: boolean | undefined
  scriptShell?: string | undefined
  stream?: boolean | undefined
  pnpmExecPath: string
  pnpmHomeDir?: string | undefined
  production?: boolean | undefined
  fetchRetries?: number | undefined
  fetchRetryFactor?: number | undefined
  fetchRetryMintimeout?: number | undefined
  fetchRetryMaxtimeout?: number | undefined
  fetchTimeout?: number | undefined
  saveExact?: boolean | undefined
  savePrefix?: string | undefined
  shellEmulator?: boolean | undefined
  scriptsPrependNodePath?: boolean | 'warn-only' | undefined
  force?: boolean | undefined
  depth?: number | undefined
  engineStrict?: boolean | undefined
  nodeVersion?: string | undefined
  offline?: boolean | undefined
  registry?: string | undefined
  optional?: boolean | undefined
  unsafePerm?: boolean | undefined
  loglevel?: 'silent' | 'error' | 'warn' | 'info' | 'debug' | undefined
  frozenLockfile?: boolean | undefined
  preferFrozenLockfile?: boolean | undefined
  only?: 'prod' | 'production' | 'dev' | 'development' | undefined
  packageManager: {
    name: string
    version: string
  }
  preferOffline?: boolean | undefined
  sideEffectsCache?: boolean | undefined // for backward compatibility
  sideEffectsCacheReadonly?: boolean | undefined // for backward compatibility
  sideEffectsCacheRead?: boolean | undefined
  sideEffectsCacheWrite?: boolean | undefined
  shamefullyHoist?: boolean | undefined
  dev?: boolean | undefined
  ignoreCurrentPrefs?: boolean | undefined
  recursive?: boolean | undefined
  enablePrePostScripts?: boolean | undefined
  useNodeVersion?: string | undefined
  useStderr?: boolean | undefined
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp' | undefined
  preferSymlinkedExecutables?: boolean | undefined
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct' | undefined
  registrySupportsTimeField?: boolean | undefined
  failedToLoadBuiltInConfig: boolean
  resolvePeersFromWorkspaceRoot?: boolean | undefined
  deployAllFiles?: boolean | undefined
  reporterHidePrefix?: boolean | undefined

  // proxy
  httpProxy?: string | undefined
  httpsProxy?: string | undefined
  localAddress?: string | undefined
  noProxy?: string | boolean | undefined

  // ssl
  cert?: string | string[] | undefined
  key?: string | undefined
  ca?: string | string[] | undefined
  strictSsl?: boolean | undefined

  userAgent?: string | undefined
  tag?: string | undefined
  updateNotifier?: boolean | undefined

  // pnpm specific configs
  cacheDir?: string | undefined
  configDir: string
  stateDir: string
  storeDir?: string | undefined
  virtualStoreDir?: string | undefined
  verifyStoreIntegrity?: boolean | undefined
  maxSockets?: number | undefined
  networkConcurrency?: number | undefined
  fetchingConcurrency?: number | undefined
  lockfileOnly?: boolean | undefined // like npm's --package-lock-only
  childConcurrency?: number | undefined
  ignorePnpmfile?: boolean | undefined
  pnpmfile: string
  hooks?: Hooks | undefined
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy' | undefined
  hoistPattern?: string[] | undefined
  publicHoistPattern?: string[] | string | undefined
  hoistWorkspacePackages?: boolean | undefined
  useStoreServer?: boolean | undefined
  useRunningStoreServer?: boolean | undefined
  workspaceConcurrency: number
  workspaceDir?: string | undefined
  reporter?: string | undefined
  aggregateOutput: boolean
  linkWorkspacePackages: boolean | 'deep'
  preferWorkspacePackages: boolean
  reverse: boolean
  sort: boolean
  strictPeerDependencies: boolean
  lockfileDir?: string | undefined
  modulesDir?: string | undefined
  sharedWorkspaceLockfile?: boolean | undefined
  useLockfile: boolean
  useGitBranchLockfile: boolean
  mergeGitBranchLockfiles?: boolean | undefined
  mergeGitBranchLockfilesBranchPattern?: string[] | undefined
  globalPnpmfile?: string | undefined
  npmPath?: string | undefined
  gitChecks?: boolean | undefined
  publishBranch?: string | undefined
  recursiveInstall?: boolean | undefined
  symlink: boolean
  enablePnp?: boolean | undefined
  enableModulesDir: boolean
  modulesCacheMaxAge: number
  embedReadme?: boolean | undefined
  gitShallowHosts?: string[] | undefined
  legacyDirFiltering?: boolean | undefined
  onlyBuiltDependencies?: string[] | undefined
  dedupePeerDependents?: boolean | undefined
  patchesDir?: string | undefined
  ignoreWorkspaceCycles?: boolean | undefined
  disallowWorkspaceCycles?: boolean | undefined
  packGzipLevel?: number | undefined

  registries: Registries
  ignoreWorkspaceRootCheck: boolean
  workspaceRoot: boolean

  testPattern?: string[] | undefined
  changedFilesIgnorePattern?: string[] | undefined
  rootProjectManifestDir: string
  rootProjectManifest?: ProjectManifest | undefined
  userConfig: Record<string, string>

  globalconfig: string
  hoist: boolean
  packageLock: boolean
  pending: boolean
  userconfig: string
  workspacePrefix?: string | undefined
  dedupeDirectDeps?: boolean | undefined
  extendNodePath?: boolean | undefined
  gitBranchLockfile?: boolean | undefined
  globalDir?: string | undefined
  lockfile?: boolean | undefined
  dedupeInjectedDeps?: boolean | undefined
}

export type StrictRebuildOptions = {
  allProjects?: Project[] | undefined
  autoInstallPeers?: boolean | undefined
  cacheDir?: string | undefined
  childConcurrency: number
  excludeLinksFromLockfile?: boolean | undefined
  extraBinPaths?: string[] | undefined
  extraEnv?: Record<string, string> | undefined
  lockfileDir: string
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  preferSymlinkedExecutables?: boolean | undefined
  scriptShell?: string | undefined
  sideEffectsCacheRead: boolean
  sideEffectsCacheWrite?: boolean | undefined
  scriptsPrependNodePath: boolean | 'warn-only'
  shellEmulator: boolean
  skipIfHasSideEffectsCache?: boolean | undefined
  storeDir: string // TODO: remove this property
  storeController?: StoreController | undefined
  force: boolean
  forceSharedLockfile: boolean
  useLockfile: boolean
  registries: Registries
  dir: string
  pnpmHomeDir?: string | undefined

  reporter?: ((logObj: LogBase) => void) | undefined
  production: boolean
  development: boolean
  optional: boolean
  rawConfig: object
  userConfig?: Record<string, string> | undefined
  userAgent: string
  packageManager: {
    name: string
    version: string
  }
  unsafePerm: boolean
  pending: boolean
  shamefullyHoist: boolean
  deployAllFiles?: boolean | undefined
  neverBuiltDependencies?: string[] | undefined
  onlyBuiltDependencies?: string[] | undefined
}

export type RebuildOptions = Partial<StrictRebuildOptions> &
  Pick<StrictRebuildOptions, 'storeDir' | 'storeController'> &
  Pick<Config, 'rootProjectManifest' | 'rootProjectManifestDir'>

export type ConfigWithDeprecatedSettings = Config & {
  globalPrefix?: string | undefined
  proxy?: string | undefined
  shamefullyFlatten?: boolean | undefined
}

export type GetHoistedDependenciesOpts = {
  lockfile: Lockfile
  importerIds?: string[]
  privateHoistPattern: string[]
  privateHoistedModulesDir: string
  publicHoistPattern: string[]
  publicHoistedModulesDir: string
  hoistedWorkspacePackages?: Record<string, HoistedWorkspaceProject>
}

export type HoistedWorkspaceProject = {
  name: string
  dir: string
}

export type PnpmContext = {
  currentLockfile: Lockfile
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  extraBinPaths: string[]
  extraNodePaths: string[]
  lockfileHadConflicts: boolean
  hoistedDependencies: HoistedDependencies
  include: IncludedDependencies
  modulesFile: Modules | null
  pendingBuilds: string[]
  projects: Record<
    string,
    {
      modulesDir: string
      id: string
    } & HookOptions &
      ProjectOptions
  >
  rootModulesDir: string
  hoistPattern: string[] | undefined
  hoistedModulesDir: string
  publicHoistPattern: string[] | undefined
  lockfileDir: string
  virtualStoreDir: string
  skipped: Set<string>
  storeDir: string
  wantedLockfile: Lockfile
  wantedLockfileIsModified: boolean
  registries: Registries
}

export type GetContextOptions = {
  autoInstallPeers?: boolean | undefined
  excludeLinksFromLockfile: boolean
  allProjects?: Array<ProjectOptions & HookOptions> | undefined
  confirmModulesPurge?: boolean | undefined
  force: boolean
  forceNewModules?: boolean | undefined
  forceSharedLockfile: boolean
  frozenLockfile?: boolean | undefined
  extraBinPaths?: string[] | undefined
  extendNodePath?: boolean | undefined
  lockfileDir: string
  modulesDir?: string | undefined
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  readPackageHook?: ReadPackageHook | undefined
  include?: IncludedDependencies | undefined
  registries: Registries
  storeDir: string
  useLockfile: boolean
  useGitBranchLockfile?: boolean | undefined
  mergeGitBranchLockfiles?: boolean | undefined
  virtualStoreDir?: string | undefined

  hoistPattern?: string[] | undefined
  forceHoistPattern?: boolean | undefined

  publicHoistPattern?: string[] | undefined
  forcePublicHoistPattern?: boolean | undefined
  global?: boolean | undefined
}

export type ImporterToPurge = {
  modulesDir: string
  rootDir: string
}

export type PnpmSingleContext = {
  currentLockfile: Lockfile
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  extraBinPaths: string[]
  extraNodePaths: string[]
  lockfileHadConflicts: boolean
  hoistedDependencies: HoistedDependencies
  hoistedModulesDir: string
  hoistPattern?: string[] | undefined
  manifest?: ProjectManifest | undefined
  modulesDir: string
  importerId: string
  prefix: string
  include: IncludedDependencies
  modulesFile: Modules | null
  pendingBuilds: string[]
  publicHoistPattern?: string[] | undefined
  registries: Registries
  rootModulesDir: string
  lockfileDir: string
  virtualStoreDir: string
  skipped: Set<string>
  storeDir: string
  wantedLockfile: Lockfile
  wantedLockfileIsModified: boolean
}

export type ImporterToUpdate = {
  buildIndex?: number | undefined
  binsDir?: string | undefined
  id: string
  manifest?: ProjectManifest | undefined
  originalManifest?: ProjectManifest | undefined
  modulesDir: string
  rootDir: string
  pruneDirectDependencies: boolean
  removePackages?: string[] | undefined
  updatePackageManifest: boolean
  wantedDependencies: Array<
    WantedDependency & {
      isNew?: boolean | undefined
      updateSpec?: boolean | undefined
      preserveNonSemverVersionSpec?: boolean | undefined
    }
  >
} & DependenciesMutation

export type InstallFunctionResult = {
  newLockfile: Lockfile
  projects: UpdatedProject[]
  stats?: InstallationResultStats | undefined
}

export type InstallFunction = (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: Omit<ProcessedInstallOptions, 'patchedDependencies'> & {
    patchedDependencies?: Record<string, PatchFile> | undefined
    makePartialCurrentLockfile: boolean
    needsFullResolution: boolean
    neverBuiltDependencies?: string[] | undefined
    onlyBuiltDependencies?: string[] | undefined
    overrides?: Record<string, string> | undefined
    updateLockfileMinorVersion: boolean
    preferredVersions?: PreferredVersions | undefined
    pruneVirtualStore: boolean
    scriptsOpts: RunLifecycleHooksConcurrentlyOptions
    currentLockfileIsUpToDate: boolean
    hoistWorkspacePackages?: boolean | undefined
  }
) => Promise<InstallFunctionResult>

export type DepsGraph = {
  [depPath: string]: DepsGraphNode
}

export type DepsGraphNode = {
  children: { [alias: string]: string }
  depPath: string
}

export type DepsStateCache = {
  [depPath: string]: DepStateObj
}

export type DepStateObj = {
  [depPath: string]: DepStateObj
}

export type PkgNameVersion = {
  name?: string | undefined
  version?: string | undefined
}

export type TarballExtractMessage = {
  type: 'extract'
  buffer: Buffer
  cafsDir: string
  integrity?: string | undefined
  filesIndexFile: string
  readManifest?: boolean | undefined
  pkg?: PkgNameVersion | undefined
}

export type LinkPkgMessage = {
  type: 'link'
  storeDir: string
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy' | undefined
  filesResponse?: PackageFilesResponse | undefined
  sideEffectsCacheKey?: string | undefined
  targetDir: string
  requiresBuild?: boolean | undefined
  force: boolean
  keepModulesDir?: boolean | undefined
  disableRelinkLocalDirDeps?: boolean | undefined
}

export type SymlinkAllModulesMessage = {
  type: 'symlinkAllModules'
  deps: Array<{
    children: Record<string, string | undefined> | undefined
    modules?: string | undefined
    name?: string | undefined
  }>
}

export type AddDirToStoreMessage = {
  type: 'add-dir'
  cafsDir: string
  dir: string
  filesIndexFile: string
  sideEffectsCacheKey?: string | undefined
  readManifest?: boolean | undefined
  pkg?: PkgNameVersion | undefined
}

export type ReadPkgFromCafsMessage = {
  type: 'readPkgFromCafs'
  cafsDir: string
  filesIndexFile: string
  readManifest: boolean
  verifyStoreIntegrity: boolean
}

export type HardLinkDirMessage = {
  type: 'hardLinkDir'
  src: string
  destDirs: string[]
}

export type DependenciesGraphNode = {
  id?: string | undefined
  alias?: string | undefined // this is populated in HoistedDepGraphOnly
  children?: Record<string, string | undefined> | undefined
  depPath: string // this option is only needed for saving pendingBuild when running with --ignore-scripts flag
  name?: string | undefined
  dir?: string | undefined
  fetchingBundledManifest?: (() => Promise<PackageManifest | undefined>) | undefined
  filesIndexFile?: string | undefined
  hasBin?: boolean | undefined
  hasBundledDependencies?: boolean | undefined
  installable?: boolean | undefined
  isBuilt?: boolean | undefined
  optionalDependencies?: Set<string> | undefined
  requiresBuild?: boolean | SafePromiseDefer<boolean> | undefined // this is a dirty workaround added in https://github.com/pnpm/pnpm/pull/4898
  patchFile?: PatchFile | undefined
  modules?: string | undefined
  depth?: number | undefined
  peerDependencies?: PeerDependencies | undefined
  transitivePeerDependencies?: Set<string> | undefined
  isPure?: boolean | undefined
  resolvedPeerNames?: Set<string> | undefined
  resolution?: Resolution | undefined
  prod?: boolean | undefined
  dev?: boolean | undefined
  optional?: boolean | undefined
  fetching?: (() => Promise<PkgRequestFetchResult>) | undefined
  version?: string | undefined
  prepare?: boolean | undefined
  additionalInfo?: {
    deprecated?: string | undefined
    bundleDependencies?: string[] | boolean | undefined
    bundledDependencies?: string[] | boolean | undefined
    engines?: {
      node?: string | undefined
      npm?: string | undefined
    } | undefined
    cpu?: string[] | undefined
    os?: string[] | undefined
    libc?: string[] | undefined
  } | undefined
  parentImporterIds?: Set<string> | undefined
}

// export type DependenciesGraph = {
//   [depPath: string]: DependenciesGraphNode
// }
export type DependenciesGraph = GenericDependenciesGraph<DependenciesGraphNode>

export type LockfileToDepGraphOptions = {
  autoInstallPeers: boolean
  engineStrict: boolean
  force: boolean
  importerIds: string[]
  include: IncludedDependencies
  ignoreScripts: boolean
  lockfileDir: string
  nodeVersion: string
  pnpmVersion: string
  patchedDependencies?: Record<string, PatchFile>
  registries: Registries
  sideEffectsCacheRead: boolean
  skipped: Set<string>
  storeController: StoreController
  storeDir: string
  virtualStoreDir: string
  supportedArchitectures?: SupportedArchitectures
}

export type DirectDependenciesByImporterId = {
  [importerId: string]: { [alias: string]: string | undefined }
}

export type DepHierarchy = {
  [depPath: string]: Record<string, DepHierarchy>
}

export type LockfileToDepGraphResult = {
  directDependenciesByImporterId: DirectDependenciesByImporterId
  graph: GenericDependenciesGraph<DependenciesGraphNode>
  hierarchy?: DepHierarchy
  hoistedLocations?: Record<string, string[]>
  symlinkedDirectDependenciesByImporterId?: DirectDependenciesByImporterId
  prevGraph?: DependenciesGraph
  pkgLocationsByDepPath?: Record<string, string[]>
}

export type IncludedDependencies = {
  [dependenciesField in DependenciesField]: boolean
}

export type Modules = {
  hoistedAliases?: ({ [depPath: string]: string[] }) | undefined // for backward compatibility
  hoistedDependencies?: HoistedDependencies | undefined
  hoistPattern?: string[] | undefined
  included: IncludedDependencies
  layoutVersion: number
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp' | undefined
  packageManager: string
  pendingBuilds: string[]
  prunedAt: string
  registries?: Registries | undefined // nullable for backward compatibility
  shamefullyHoist?: boolean | undefined // for backward compatibility
  publicHoistPattern?: string[] | undefined
  skipped: string[]
  storeDir: string
  virtualStoreDir: string
  injectedDeps?: Record<string, string[]> | undefined
  hoistedLocations?: Record<string, string[]> | undefined
}

export type RunLifecycleHookOptions = {
  args?: string[] | undefined
  depPath: string
  extraBinPaths?: string[] | undefined
  extraEnv?: Record<string, string> | undefined
  initCwd?: string | undefined
  optional?: boolean | undefined
  pkgRoot: string
  rawConfig: object
  rootModulesDir?: string | undefined
  scriptShell?: string | undefined
  silent?: boolean | undefined
  scriptsPrependNodePath?: boolean | 'warn-only' | undefined
  shellEmulator?: boolean | undefined
  stdio?: string | undefined
  unsafePerm: boolean
}

export type RunLifecycleHooksConcurrentlyOptions = Omit<
  RunLifecycleHookOptions,
  'depPath' | 'pkgRoot' | 'rootModulesDir'
> & {
  resolveSymlinksInInjectedDirs?: boolean | undefined
  storeController: StoreController
  extraNodePaths?: string[] | undefined
  preferSymlinkedExecutables?: boolean | undefined
}

export type LockedDependency = {
  depPath: string
  pkgSnapshot: PackageSnapshot
  next: () => LockfileWalkerStep
}

export type LockfileWalkerStep = {
  dependencies: LockedDependency[]
  links: string[]
  missing: string[]
}

export type DirectDep = {
  alias: string
  depPath: string
}

export type ParsedWantedDependency = {
  alias: string
  pref: string
}

export type ApplyPatchToDirOpts = {
  patchedDir: string
  patchFilePath: string
}

export type ResolverFactoryOptions = {
  cacheDir: string
  timeout?: number | undefined
  offline?: boolean | undefined
  fullMetadata?: boolean | undefined
  preferOffline?: boolean | undefined
  filterMetadata?: boolean | undefined
  retry?: RetryTimeoutOptions | undefined
}

export type ResolveFromNpmOptions = {
  registry: string
  dryRun?: boolean | undefined
  publishedBy?: Date | undefined
  defaultTag?: string | undefined
  projectDir?: string | undefined
  lockfileDir?: string | undefined
  updateToLatest?: boolean | undefined
  pickLowestVersion?: boolean | undefined
  preferWorkspacePackages?: boolean | undefined
  alwaysTryWorkspacePackages?: boolean | undefined
  workspacePackages?: WorkspacePackages | undefined
  preferredVersions?: PreferredVersions | undefined
}

export type ClientOptions = {
  authConfig: Record<string, string>
  customFetchers?: CustomFetchers | undefined
  ignoreScripts?: boolean | undefined
  rawConfig: object
  retry?: RetryTimeoutOptions | undefined
  timeout?: number | undefined
  unsafePerm?: boolean | undefined
  userAgent?: string | undefined
  userConfig?: Record<string, string> | undefined
  gitShallowHosts?: string[] | undefined
  resolveSymlinksInInjectedDirs?: boolean | undefined
  includeOnlyPackageFiles?: boolean | undefined
} & ResolverFactoryOptions &
  AgentOptions

export interface Client {
  fetchers: Fetchers
  resolve: ResolveFunction
}

export type CafsLocker = Map<string, number>

export interface CreateCafsOpts {
  ignoreFile?: (filename: string) => boolean
  cafsLocker?: CafsLocker
}

export type WriteBufferToCafs = (
  buffer: Buffer,
  fileDest: string,
  mode: number | undefined,
  integrity: ssri.IntegrityLike
) => { checkedAt: number; filePath: string }

export type CreateNewStoreControllerOptions =
  Pick<
    Config,
    | 'ca'
    | 'cert'
    | 'engineStrict'
    | 'force'
    | 'nodeVersion'
    | 'fetchTimeout'
    | 'gitShallowHosts'
    | 'ignoreScripts'
    | 'hooks'
    | 'httpProxy'
    | 'httpsProxy'
    | 'key'
    | 'localAddress'
    | 'maxSockets'
    | 'networkConcurrency'
    | 'noProxy'
    | 'offline'
    | 'packageImportMethod'
    | 'preferOffline'
    | 'registry'
    | 'registrySupportsTimeField'
    | 'resolutionMode'
    | 'strictSsl'
    | 'unsafePerm'
    | 'userAgent'
    | 'verifyStoreIntegrity'
    | 'cacheDir'
    | 'fetchRetries'
    | 'fetchRetryFactor'
    | 'fetchRetryMaxtimeout'
    | 'fetchRetryMintimeout'
    | 'offline'
    | 'rawConfig'
    | 'verifyStoreIntegrity'
  > & {
    cafsLocker?: CafsLocker | undefined
    ignoreFile?: ((filename: string) => boolean) | undefined
  } & Partial<Pick<Config, 'userConfig' | 'deployAllFiles'>> &
  Required<Pick<Config, 'storeDir'>> &
  Pick<ClientOptions, 'resolveSymlinksInInjectedDirs'>

export type CreateStoreControllerOptions = Omit<
  CreateNewStoreControllerOptions,
  'storeDir'
> &
  Pick<
    Config,
    | 'storeDir'
    | 'dir'
    | 'pnpmHomeDir'
    | 'useRunningStoreServer'
    | 'useStoreServer'
    | 'workspaceDir'
  >

export type PatchCommandOptions = Pick<
  Config,
  | 'dir'
  | 'registries'
  | 'tag'
  | 'storeDir'
  | 'rootProjectManifest'
  | 'lockfileDir'
  | 'modulesDir'
  | 'virtualStoreDir'
  | 'sharedWorkspaceLockfile'
> &
  CreateStoreControllerOptions & {
    editDir?: string
    reporter?: (logObj: LogBase) => void
    ignoreExisting?: boolean
  }

export type GetPatchedDependencyOptions = {
  lockfileDir: string
} & Pick<Config, 'virtualStoreDir' | 'modulesDir'>

export type ParseWantedDependencyResult = Partial<ParsedWantedDependency> &
  (
    | Omit<ParsedWantedDependency, 'pref'>
    | Omit<ParsedWantedDependency, 'alias'>
    | ParsedWantedDependency
  )

export type ReporterType = 'default' | 'ndjson' | 'silent' | 'append-only'

export type PnpmOptions = Omit<Config, 'reporter'> & {
  argv: {
    cooked: string[]
    original: string[]
    remain: string[]
  }
  cliOptions: object
  reporter?: (logObj: LogBase) => void
  packageManager?: {
    name: string
    version: string
  }

  hooks?: {
    readPackage?: ReadPackageHook[]
  }

  ignoreFile?: (filename: string) => boolean
}

// export type GenericDependenciesGraphNode = {
//   // at this point the version is really needed only for logging
//   modules: string
//   dir: string
//   children: Record<string, string>
//   depth: number
//   peerDependencies?: PeerDependencies | undefined
//   transitivePeerDependencies: Set<string>
//   installable?: boolean | undefined
//   isBuilt?: boolean | undefined
//   isPure: boolean
//   resolvedPeerNames: Set<string>
// }

// export type ResolvedPackage = {
//   id: string
//   resolution: Resolution
//   prod: boolean
//   dev?: boolean | undefined
//   optional?: boolean | undefined
//   fetching?: (() => Promise<PkgRequestFetchResult>) | undefined
//   filesIndexFile: string
//   name?: string | undefined
//   version?: string | undefined
//   peerDependencies: PeerDependencies
//   optionalDependencies: Set<string>
//   hasBin: boolean
//   hasBundledDependencies: boolean
//   patchFile?: PatchFile | undefined
//   prepare: boolean
//   depPath: string
//   requiresBuild: boolean | SafePromiseDefer<boolean>
//   additionalInfo: {
//     deprecated?: string | undefined
//     bundleDependencies?: string[] | boolean | undefined
//     bundledDependencies?: string[] | boolean | undefined
//     engines?: {
//       node?: string | undefined
//       npm?: string | undefined
//     } | undefined
//     cpu?: string[] | undefined
//     os?: string[] | undefined
//     libc?: string[] | undefined
//   }
//   parentImporterIds: Set<string>
// }

// export type DependenciesGraph = {
//   [depPath: string]: DependenciesGraphNode
// }

// export type DependenciesGraphNode = GenericDependenciesGraphNode &
//   ResolvedPackage

export type PartialResolvedPackage = Pick<
  DependenciesGraphNode,
  'id' | 'depPath' | 'name' | 'peerDependencies' | 'version'
>

export type GenericDependenciesGraph<T extends PartialResolvedPackage> = {
  [depPath: string]: T & DependenciesGraphNode
}

export type ProjectToResolve = {
  directNodeIdsByAlias: { [alias: string]: string }
  // only the top dependencies that were already installed
  // to avoid warnings about unresolved peer dependencies
  topParents: Array<{ name?: string | undefined; version?: string | undefined; alias?: string | undefined; linkedDir?: string | undefined }>
  rootDir: string // is only needed for logging
  id: string
}

export type DependenciesByProjectId = Record<string, Record<string, string>>

export type PeersCacheItem = {
  depPath: string
  resolvedPeers: Map<string, string>
  missingPeers: Set<string>
}

export type PeersCache = Map<string, PeersCacheItem[]>

export type PeersResolution = {
  missingPeers: Set<string>
  resolvedPeers: Map<string, string>
}

export type ResolvePeersContext = {
  pathsByNodeId: Map<string, string>
  depPathsByPkgId?: Map<string, Set<string>>
}

export type ParentRefs = {
  [name: string]: ParentRef
}

export type VersionSpecsByRealNames = Record<
  string,
  Record<string, 'version' | 'range' | 'tag'>
>

export type ParentRef = {
  version?: string | undefined
  depth?: number | undefined
  // this is null only for already installed top dependencies
  nodeId?: string | undefined
  alias?: string | undefined
}

export type ProjectToLink = {
  directNodeIdsByAlias: { [alias: string]: string }
  topParents: Array<{ name?: string | undefined; version?: string | undefined; alias?: string | undefined; linkedDir?: string | undefined }>
  binsDir?: string | undefined
  id: string
  linkedDependencies: LinkedDependency[]
  manifest?: ProjectManifest | undefined
  modulesDir: string
  rootDir: string
}

export type Importer<T> = {
  id: string
  rootDir: string
  modulesDir: string
  wantedDependencies: Array<T & WantedDependency>
  manifest?: ProjectManifest | undefined
  removePackages?: string[] | undefined
}

export type LifecycleImporter = {
  buildIndex?: number | undefined
  manifest?: ProjectManifest | undefined
  rootDir: string
  modulesDir?: string | undefined
  stages?: string[] | undefined
  targetDirs?: string[] | undefined
}

export type ImporterToResolveGeneric<T> = Importer<T> & {
  updatePackageManifest: boolean
  wantedDependencies: Array<T & WantedDependency & { updateDepth: number }>
  hasRemovedDependencies?: boolean | undefined
  preferredVersions?: PreferredVersions | undefined
  updateMatching?: ((pkgName: string) => boolean) | undefined
}

export type ImporterToResolve = Importer<{
  raw: string
  isNew?: boolean | undefined
  updateSpec?: boolean | undefined
  nodeExecPath?: string | undefined
  preserveNonSemverVersionSpec?: boolean
  pinnedVersion?: PinnedVersion | undefined
}> & {
  updatePackageManifest: boolean
  peer?: boolean | undefined
  binsDir?: string | undefined
  update?: boolean | undefined
  manifest?: ProjectManifest | undefined
  pinnedVersion?: PinnedVersion | undefined
  originalManifest?: ProjectManifest | undefined
  updateMatching?: UpdateMatchingFunction | undefined
  targetDependenciesField?: DependenciesField | undefined
}
export type ImporterToResolveDeps = {
  updatePackageManifest: boolean
  preferredVersions: PreferredVersions
  parentPkgAliases: ParentPkgAliases
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>
  options: Omit<ResolvedDependenciesOptions, 'parentPkgAliases' | 'publishedBy'>
}

export type DependenciesField =
  | 'optionalDependencies'
  | 'dependencies'
  | 'devDependencies'

export type DependenciesOrPeersField = DependenciesField | 'peerDependencies'

export type Registries = {
  default: string
  [scope: string]: string
}

export type HoistedDependencies = Record<
  string,
  Record<string, 'public' | 'private'>
>

export type PatchFile = {
  path: string
  hash: string
}

export type LogBase =
  | {
    level: 'debug' | 'error'
  }
  | {
    level: 'info' | 'warn'
    prefix: string
    message: string
  }

export type ReadPackageHook = {
  (
    pkg?: PackageManifest | ProjectManifest | undefined,
    dir?: string | undefined
  ): PackageManifest | ProjectManifest | Promise<PackageManifest | ProjectManifest> | undefined
}

export type DedupeCheckIssues = {
  readonly importerIssuesByImporterId: SnapshotsChanges
  readonly packageIssuesByDepPath: SnapshotsChanges
}

export type SnapshotsChanges = {
  readonly added: readonly string[]
  readonly removed: readonly string[]
  readonly updated: Record<string, ResolutionChangesByAlias>
}

export type ResolutionChangesByAlias = Record<string, ResolutionChange>

export type ResolutionChange =
  | ResolutionAdded
  | ResolutionDeleted
  | ResolutionUpdated

export type ResolutionAdded = {
  readonly type: 'added'
  readonly next: string
}

export type ResolutionDeleted = {
  readonly type: 'removed'
  readonly prev: string
}

export type ResolutionUpdated = {
  readonly type: 'updated'
  readonly prev: string
  readonly next: string
}

export type Dependencies = Record<string, string>

export type PackageBin = string | { [name: string]: string }

export type LockfileSettings = {
  autoInstallPeers?: boolean | undefined
  excludeLinksFromLockfile?: boolean | undefined
}

export type ResolvedDependencies = Record<string, string>
/**
 * directory on a file system
 */
export type DirectoryResolution = {
  type: 'directory'
  directory: string
}

/**
 * tarball hosted remotely
 */
export type TarballResolution = {
  type?: string | undefined
  tarball: string
  integrity?: string | undefined
}

/**
 * Git repository
 */
export type GitRepositoryResolution = {
  type: 'git'
  repo: string
  commit: string
}

export type Resolution =
  | TarballResolution
  | GitRepositoryResolution
  | DirectoryResolution
  | GitResolution
  | ({ type: keyof Fetchers } & object)
export type LockfileResolution =
  | Resolution
  | {
    integrity: string
  }

export type PackageSnapshot = {
  id?: string | undefined
  dev?: boolean | undefined
  optional?: boolean | undefined
  requiresBuild?: boolean | undefined
  patched?: boolean | undefined
  prepare?: boolean | undefined
  hasBin?: boolean | undefined
  // name and version are only needed
  // for packages that are hosted not in the npm registry
  name?: string | undefined
  version?: string | undefined
  resolution?: LockfileResolution | undefined
  dependencies?: ResolvedDependencies | undefined
  optionalDependencies?: ResolvedDependencies | undefined
  peerDependencies?: {
    [name: string]: string
  } | undefined
  peerDependenciesMeta?: {
    [name: string]: {
      optional: true
    }
  } | undefined
  transitivePeerDependencies?: string[] | undefined
  bundledDependencies?: string[] | boolean | undefined
  engines?: (Record<string, string> & {
    node?: string | undefined
  }) | undefined
  os?: string[] | undefined
  cpu?: string[] | undefined
  libc?: string[] | undefined
  deprecated?: string | undefined
}

export type PackageSnapshots = Record<string, PackageSnapshot>

export type ProjectSnapshot = {
  specifiers: ResolvedDependencies
  dependencies?: ResolvedDependencies | undefined
  optionalDependencies?: ResolvedDependencies | undefined
  devDependencies?: ResolvedDependencies | undefined
  dependenciesMeta?: DependenciesMeta | undefined
  publishDirectory?: string | undefined
}

export type Lockfile = {
  importers: Record<string, ProjectSnapshot>
  lockfileVersion: number | string
  time?: Record<string, string> | undefined
  packages?: PackageSnapshots | undefined
  neverBuiltDependencies?: string[] | undefined
  onlyBuiltDependencies?: string[] | undefined
  overrides?: Record<string, string> | undefined
  packageExtensionsChecksum?: string | undefined
  patchedDependencies?: Record<string, PatchFile> | undefined
  settings?: LockfileSettings | undefined
}

export type PackageScripts = {
  [name: string]: string
} & {
  prepublish?: string | undefined
  prepare?: string | undefined
  prepublishOnly?: string | undefined
  prepack?: string | undefined
  postpack?: string | undefined
  publish?: string | undefined
  postpublish?: string | undefined
  preinstall?: string | undefined
  install?: string | undefined
  postinstall?: string | undefined
  preuninstall?: string | undefined
  uninstall?: string | undefined
  postuninstall?: string | undefined
  preversion?: string | undefined
  version?: string | undefined
  postversion?: string | undefined
  pretest?: string | undefined
  test?: string | undefined
  posttest?: string | undefined
  prestop?: string | undefined
  stop?: string | undefined
  poststop?: string | undefined
  prestart?: string | undefined
  start?: string | undefined
  poststart?: string | undefined
  prerestart?: string | undefined
  restart?: string | undefined
  postrestart?: string | undefined
  preshrinkwrap?: string | undefined
  shrinkwrap?: string | undefined
  postshrinkwrap?: string | undefined
}

export type PeerDependenciesMeta = {
  [dependencyName: string]: {
    optional?: boolean
  }
}

export type DependenciesMeta = {
  [dependencyName: string]: {
    injected?: boolean | undefined
    node?: string | undefined
    patch?: string | undefined
  }
}

export type PublishConfig = Record<string, unknown> & {
  directory?: string | undefined
  linkDirectory?: boolean | undefined
  executableFiles?: string[] | undefined
  registry?: string | undefined
}

type Version = string
type Pattern = string
export type TypesVersions = {
  [version: Version]: {
    [pattern: Pattern]: string[]
  }
}

export type BaseManifest = {
  name?: string | undefined
  version?: string | undefined
  bin?: PackageBin | undefined
  description?: string | undefined
  directories?: {
    bin?: string | undefined
  } | undefined
  files?: string[] | undefined
  dependencies?: Dependencies | undefined
  devDependencies?: Dependencies | undefined
  optionalDependencies?: Dependencies | undefined
  peerDependencies?: Dependencies | undefined
  peerDependenciesMeta?: PeerDependenciesMeta | undefined
  dependenciesMeta?: DependenciesMeta | undefined
  bundleDependencies?: string[] | boolean | undefined
  bundledDependencies?: string[] | boolean | undefined
  homepage?: string | undefined
  repository?: string | { url: string } | undefined
  scripts?: PackageScripts | undefined
  config?: object | undefined
  engines?: {
    node?: string | undefined
    npm?: string | undefined
    pnpm?: string | undefined
  } | undefined
  cpu?: string[] | undefined
  os?: string[] | undefined
  libc?: string[] | undefined
  main?: string | undefined
  module?: string | undefined
  typings?: string | undefined
  types?: string | undefined
  publishConfig?: PublishConfig | undefined
  typesVersions?: TypesVersions | undefined
  readme?: string | undefined
  keywords?: string[] | undefined
  author?: string | undefined
  license?: string | undefined
  exports?: Record<string, string> | undefined
  hasInstallScript?: boolean | undefined
}

export type DependencyManifest = BaseManifest &
  Required<Pick<BaseManifest, 'name' | 'version'>>

export type PackageExtension = Pick<
  BaseManifest,
  | 'dependencies'
  | 'optionalDependencies'
  | 'peerDependencies'
  | 'peerDependenciesMeta'
>

export type PeerDependencyRules = {
  ignoreMissing?: string[] | undefined
  allowAny?: string[] | undefined
  allowedVersions?: Record<string, string> | undefined
}

export type AllowedDeprecatedVersions = Record<string, string>

export type ProjectManifest = BaseManifest & {
  deprecated?: string | undefined
  workspaces?: string[] | undefined
  pnpm?: {
    neverBuiltDependencies?: string[] | undefined
    onlyBuiltDependencies?: string[] | undefined
    onlyBuiltDependenciesFile?: string | undefined
    overrides?: Record<string, string> | undefined
    packageExtensions?: Record<string, PackageExtension> | undefined
    peerDependencyRules?: PeerDependencyRules | undefined
    allowedDeprecatedVersions?: AllowedDeprecatedVersions | undefined
    allowNonAppliedPatches?: boolean | undefined
    patchedDependencies?: Record<string, string> | undefined
    updateConfig?: {
      ignoreDependencies?: string[] | undefined
    } | undefined
    auditConfig?: {
      ignoreCves?: string[] | undefined
    } | undefined
    requiredScripts?: string[] | undefined
    supportedArchitectures?: SupportedArchitectures | undefined
  } | undefined
  private?: boolean | undefined
  resolutions?: Record<string, string> | undefined
}

export type PackageManifest = DependencyManifest & {
  deprecated?: string | undefined
}

export type SupportedArchitectures = {
  os?: string[] | undefined
  cpu?: string[] | undefined
  libc?: string[] | undefined
}

export type ResolvedDependenciesResult = {
  pkgAddresses: Array<PkgAddress | LinkedDependency>
  resolvingPeers: Promise<PeersResolutionResult>
}

export type PkgAddressesByImportersWithoutPeers = PeersResolutionResult & {
  pkgAddresses: Array<PkgAddress | LinkedDependency>
}

export type ResolveDependenciesOfImportersResult = {
  pkgAddressesByImportersWithoutPeers: PkgAddressesByImportersWithoutPeers[]
  publishedBy?: Date | undefined
  time?: Record<string, string> | undefined
}

export type ResolvedImporters = {
  [id: string]: {
    directDependencies: ResolvedDirectDependency[]
    directNodeIdsByAlias: {
      [alias: string]: string
    }
    linkedDependencies: LinkedDependency[]
  }
}

export type ResolvedDirectDependency = {
  alias: string
  optional?: boolean | undefined
  dev?: boolean | undefined
  resolution?: Resolution | undefined
  pkgId: string
  version?: string | undefined
  name?: string | undefined
  normalizedPref?: string | undefined
}

export type ResolveDependenciesOptions = {
  autoInstallPeers?: boolean | undefined
  autoInstallPeersFromHighestMatch?: boolean | undefined
  allowBuild?: ((pkgName: string) => boolean) | undefined
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allowNonAppliedPatches: boolean
  currentLockfile: Lockfile
  dryRun: boolean
  engineStrict: boolean
  force: boolean
  forceFullResolution: boolean
  ignoreScripts?: boolean | undefined
  hooks: {
    readPackage?: ReadPackageHook | undefined
  }
  nodeVersion: string
  registries: Registries
  patchedDependencies?: Record<string, PatchFile> | undefined
  pnpmVersion: string
  preferredVersions?: PreferredVersions | undefined
  preferWorkspacePackages?: boolean | undefined
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct' | undefined
  resolvePeersFromWorkspaceRoot?: boolean | undefined
  linkWorkspacePackagesDepth?: number | undefined
  lockfileDir: string
  storeController: StoreController
  tag: string
  virtualStoreDir: string
  wantedLockfile: Lockfile
  workspacePackages: WorkspacePackages
  supportedArchitectures?: SupportedArchitectures | undefined
  updateToLatest?: boolean | undefined
}

export type PinnedVersion = 'major' | 'minor' | 'patch' | 'none'

export type WantedDependency = {
  raw?: string | undefined
  injected?: boolean | undefined
  optional?: boolean | undefined
  dev?: boolean | undefined
  nodeExecPath?: string | undefined
  pinnedVersion?: PinnedVersion | undefined
  updateSpec?: boolean | undefined
} & (
  | {
    alias?: string | undefined
    pref: string
  }
  | {
    alias: string
    pref?: string | undefined
  }
)

export type InstallMutationOptions = {
  update?: boolean | undefined
  updateMatching?: UpdateMatchingFunction | undefined
  updatePackageManifest?: boolean | undefined
}

export type InstallDepsMutation = InstallMutationOptions & {
  mutation: 'install'
  pruneDirectDependencies?: boolean | undefined
}

export type AddDependenciesToPackageOptions = Omit<InstallOptions, 'allProjects'> & {
  bin?: string | undefined
  allowNew?: boolean | undefined
  peer?: boolean | undefined
  pinnedVersion?: 'major' | 'minor' | 'patch' | undefined
  targetDependenciesField?: DependenciesField | undefined
} & InstallMutationOptions

export type InstallSomeDepsMutation = InstallMutationOptions & {
  allowNew?: boolean | undefined
  dependencySelectors: string[]
  mutation: 'installSome'
  peer?: boolean | undefined
  pruneDirectDependencies?: boolean | undefined
  pinnedVersion?: PinnedVersion | undefined
  targetDependenciesField?: DependenciesField | undefined
}

export type UninstallSomeDepsMutation = {
  mutation: 'uninstallSome'
  dependencyNames: string[]
  targetDependenciesField?: DependenciesField | undefined
}

export type UnlinkDepsMutation = {
  mutation: 'unlink'
}

export type UnlinkSomeDepsMutation = {
  mutation: 'unlinkSome'
  dependencyNames: string[]
}

export type DependenciesMutation =
  | InstallDepsMutation
  | InstallSomeDepsMutation
  | UninstallSomeDepsMutation
  | UnlinkDepsMutation
  | UnlinkSomeDepsMutation

export type ProjectToBeInstalled = {
  id: string
  buildIndex: number
  manifest: ProjectManifest
  modulesDir?: string | undefined
  rootDir: string
}

export type ActionFailure = {
  status: 'failure'
  duration?: number | undefined
  prefix: string
  message: string
  error: Error
}

export type ActionPassed = {
  status: 'passed'
  duration?: number | undefined
}

export type ActionQueued = {
  status: 'queued'
}

export type ActionRunning = {
  status: 'running'
  duration?: number | undefined
}

export type ActionSkipped = {
  status: 'skipped'
}

export type Actions =
  | ActionPassed
  | ActionQueued
  | ActionRunning
  | ActionSkipped
  | ActionFailure

export type RecursiveSummary = Record<string, Actions>

export type Completion = {
  name: string
  description?: string
}

export type CompletionFunc = (
  options: Record<string, unknown>,
  params: string[]
) => Promise<Completion[]>

export type MutatedProject = DependenciesMutation & { rootDir: string; modulesDir?: string | undefined }

export type MutateModulesOptions = InstallOptions & {
  preferredVersions?: PreferredVersions | undefined
  hooks?:
    | {
      readPackage?: ReadPackageHook[] | ReadPackageHook
    }
    | InstallOptions['hooks']
    | undefined

}

export type ReadProjectManifestOpts = {
  engineStrict?: boolean | undefined
  nodeVersion?: string | undefined
  supportedArchitectures?: SupportedArchitectures | undefined
}

export type BaseReadProjectManifestResult = {
  fileName: string
  writeProjectManifest: (
    manifest: ProjectManifest,
    force?: boolean | undefined
  ) => Promise<void>
}

export type ReadProjectManifestResult = BaseReadProjectManifestResult & {
  manifest: ProjectManifest
}

export type RawLockfile = LockfileV6 & Partial<ProjectSnapshotV6>

export type AssertedProject = {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  requireModule: (moduleName: string) => any
  dir: () => string
  has: (pkgName: string, modulesDir?: string | undefined) => Promise<void>
  hasNot: (pkgName: string, modulesDir?: string | undefined) => Promise<void>
  getStorePath: () => Promise<string>
  resolve: (
    pkgName: string,
    version?: string | undefined,
    relativePath?: string | undefined
  ) => Promise<string>
  getPkgIndexFilePath: (pkgName: string, version?: string | undefined) => Promise<string>
  cafsHas: (pkgName: string, version?: string | undefined) => Promise<void>
  cafsHasNot: (pkgName: string, version?: string | undefined) => Promise<void>
  storeHas: (pkgName: string, version?: string | undefined) => Promise<string>
  storeHasNot: (pkgName: string, version?: string | undefined) => Promise<void>
  isExecutable: (pathToExe: string) => Promise<void>
  /**
   * TODO: Remove the `Required<T>` cast.
   *
   * https://github.com/microsoft/TypeScript/pull/32695 might help with this.
   */
  readCurrentLockfile: () => Promise<Required<RawLockfile>>
  readModulesManifest: () => Promise<Modules | null>
  /**
   * TODO: Remove the `Required<T>` cast.
   *
   * https://github.com/microsoft/TypeScript/pull/32695 might help with this.
   */
  readLockfile: (lockfileName?: string | undefined) => Promise<Required<RawLockfile>>
  writePackageJson: (pkgJson: JsonObject) => Promise<void>
}

export type CommandError = Error & {
  originalMessage: string
  shortMessage: string
}

export type WriteProjectManifest = (
  manifest: ProjectManifest,
  force?: boolean
) => Promise<void>

export type Project = {
  dir: string
  binsDir?: string | undefined
  buildIndex?: number | undefined
  manifest?: ProjectManifest | undefined
  modulesDir: string
  id: string
  pruneDirectDependencies?: boolean | undefined
  rootDir: string
  writeProjectManifest: (
    manifest: ProjectManifest | MutateModulesResult | undefined,
    force?: boolean | undefined
  ) => Promise<void>
}

export type ProjectsGraph = Record<
  string,
  { dependencies: string[]; package: Project }
>
export type PreResolutionHookContext = {
  wantedLockfile: Lockfile
  currentLockfile: Lockfile
  existsCurrentLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  lockfileDir: string
  storeDir: string
  registries: Registries
}

export type PreResolutionHookLogger = {
  info: (message: string) => void
  warn: (message: string) => void
}

export type PreResolutionHook = (
  ctx: PreResolutionHookContext,
  logger: PreResolutionHookLogger
) => Promise<void>

export type StrictInstallOptions = {
  autoInstallPeers?: boolean | undefined
  autoInstallPeersFromHighestMatch: boolean
  forceSharedLockfile: boolean
  frozenLockfile: boolean
  frozenLockfileIfExists?: boolean | undefined
  enablePnp: boolean
  extraBinPaths?: string[] | undefined
  extraEnv?: Record<string, string> | undefined
  hoistingLimits?: HoistingLimits | undefined
  externalDependencies?: Set<string> | undefined
  useLockfile: boolean
  saveLockfile: boolean
  useGitBranchLockfile: boolean
  mergeGitBranchLockfiles: boolean
  linkWorkspacePackagesDepth?: number | undefined
  lockfileOnly: boolean
  forceFullResolution: boolean
  fixLockfile?: boolean | undefined
  dedupe?: boolean | undefined
  ignoreCompatibilityDb: boolean
  ignoreDepScripts: boolean
  ignorePackageManifest: boolean
  preferFrozenLockfile: boolean
  saveWorkspaceProtocol: boolean | 'rolling'
  lockfileCheck?: ((prev: Lockfile, next: Lockfile) => void) | undefined
  lockfileIncludeTarballUrl: boolean
  preferWorkspacePackages: boolean
  preserveWorkspaceProtocol: boolean
  scriptsPrependNodePath: boolean | 'warn-only'
  scriptShell?: string | undefined
  shellEmulator: boolean
  storeController: StoreController
  storeDir: string
  reporter?: ReporterFunction | undefined
  force: boolean
  forcePublicHoistPattern?: boolean | undefined
  depth: number
  lockfileDir: string
  modulesDir?: string | undefined
  rawConfig: Record<string, any> // eslint-disable-line @typescript-eslint/no-explicit-any
  verifyStoreIntegrity: boolean
  engineStrict: boolean
  neverBuiltDependencies?: string[] | undefined
  onlyBuiltDependencies?: string[] | undefined
  onlyBuiltDependenciesFile?: string | undefined
  nodeExecPath?: string | undefined
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  nodeVersion: string
  packageExtensions: Record<string, PackageExtension>
  packageManager: {
    name: string
    version: string
  }
  pruneLockfileImporters: boolean
  hooks: {
    readPackage?: ReadPackageHook[] | undefined
    preResolution?: ((ctx: PreResolutionHookContext) => Promise<void> | undefined)
    afterAllResolved?: (Array<
      (lockfile: Lockfile) => Lockfile | Promise<Lockfile>
    > | undefined)
  }
  sideEffectsCacheRead: boolean
  sideEffectsCacheWrite: boolean
  strictPeerDependencies: boolean
  include: IncludedDependencies
  includeDirect: IncludedDependencies
  ignoreCurrentPrefs: boolean
  ignoreScripts: boolean
  childConcurrency: number
  userAgent: string
  unsafePerm: boolean
  registries: Registries
  tag: string
  updateToLatest?: boolean | undefined
  overrides: Record<string, string>
  ownLifecycleHooksStdio: 'inherit' | 'pipe'
  workspacePackages: WorkspacePackages
  pruneStore: boolean
  virtualStoreDir?: string | undefined
  dir?: string | undefined
  symlink: boolean
  enableModulesDir: boolean
  modulesCacheMaxAge: number
  peerDependencyRules?: PeerDependencyRules | undefined
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allowNonAppliedPatches: boolean
  preferSymlinkedExecutables?: boolean | undefined
  resolutionMode: 'highest' | 'time-based' | 'lowest-direct'
  resolvePeersFromWorkspaceRoot: boolean
  ignoreWorkspaceCycles: boolean
  disallowWorkspaceCycles: boolean

  publicHoistPattern: string[] | undefined
  hoistPattern: string[] | undefined
  forceHoistPattern?: boolean | undefined

  shamefullyHoist: boolean
  forceShamefullyHoist?: boolean | undefined

  global?: boolean | undefined
  globalBin?: string | undefined
  patchedDependencies?: Record<string, string>

  allProjects?: ProjectOptions[] | undefined
  resolveSymlinksInInjectedDirs: boolean
  dedupeDirectDeps: boolean
  dedupeInjectedDeps?: boolean | undefined
  dedupePeerDependents: boolean
  extendNodePath: boolean
  excludeLinksFromLockfile: boolean
  confirmModulesPurge: boolean
  /**
   * Don't relink local directory dependencies if they are not hard linked from the local directory.
   *
   * This option was added to fix an issue with Bit CLI.
   * Bit compile adds dist directories to the injected dependencies, so if pnpm were to relink them,
   * the dist directories would be deleted.
   *
   * The option might be used in the future to improve performance.
   */
  disableRelinkLocalDirDeps?: boolean | undefined

  supportedArchitectures?: SupportedArchitectures
  hoistWorkspacePackages?: boolean
}

export type InstallOptions = Partial<StrictInstallOptions> &
  Pick<StrictInstallOptions, 'storeDir' | 'storeController'>

export type ProcessedInstallOptions = StrictInstallOptions & {
  readPackageHook?: ReadPackageHook | undefined
  forceNewModules?: boolean | undefined
}

export type PackageFileInfo = {
  checkedAt?: number | undefined // Nullable for backward compatibility
  integrity: string
  mode: number
  size: number
}

export type SkippedOptionalDependencyLog = {
  name: 'pnpm:skipped-optional-dependency'
} & LogBase &
  SkippedOptionalDependencyMessage

export type SkippedOptionalDependencyMessage = {
  details?: string | undefined
  parents?: Array<{ id?: string | undefined; name?: string | undefined; version?: string | undefined }> | undefined
  prefix: string
} & (
  | {
    package: {
      id: string
      name: string
      version: string
    }
    reason: 'unsupported_engine' | 'unsupported_platform' | 'build_failure'
  }
  | {
    package: {
      name?: string | undefined
      version?: string | undefined
      pref?: string | undefined
    }
    reason: 'resolution_failure'
  }
)

export type ResolvedFrom = 'store' | 'local-dir' | 'remote'

export type PackageFilesResponse = {
  resolvedFrom: ResolvedFrom
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  sideEffects?: Record<string, Record<string, PackageFileInfo>>
} & (
  | {
    unprocessed?: false | undefined
    filesIndex: Record<string, string>
  }
  | {
    unprocessed: true
    filesIndex: Record<string, PackageFileInfo>
  }
)

export type InfoFromLockfile = {
  depPath: string
  pkgId: string
  dependencyLockfile?: PackageSnapshot | undefined
  name?: string | undefined
  version?: string | undefined
  resolution?: Resolution | undefined
} & (
  | {
    dependencyLockfile: PackageSnapshot
    name: string
    version: string
    resolution: Resolution
  }
  | unknown
)

export type ExtendedWantedDependency = {
  infoFromLockfile?: InfoFromLockfile
  proceed: boolean
  wantedDependency: WantedDependency & { updateDepth?: number }
}

export type ResolveDependenciesOfDependency = {
  postponedResolution?: PostponedResolutionFunction
  postponedPeersResolution?: PostponedPeersResolutionFunction
  resolveDependencyResult: ResolveDependencyResult
}

export type ImportPackageOpts = {
  disableRelinkLocalDirDeps?: boolean | undefined
  requiresBuild?: boolean | undefined
  sideEffectsCacheKey?: string | undefined
  filesResponse?: PackageFilesResponse | undefined
  force: boolean
  keepModulesDir?: boolean | undefined
}

export type ImportPackageFunction = (
  to: string,
  opts: ImportPackageOpts
) => { isBuilt: boolean; importMethod?: undefined | string }

export type ImportPackageFunctionAsync = (
  to: string,
  opts: ImportPackageOpts
) => Promise<{ isBuilt: boolean; importMethod?: undefined | string }>

export type FileType = 'exec' | 'nonexec' | 'index'

export type FilesIndex = {
  [filename: string]: {
    mode: number
    size: number
  } & FileWriteResult
}

export type FileWriteResult = {
  checkedAt: number
  filePath: string
  integrity: IntegrityLike
}

export type AddToStoreResult = {
  filesIndex: FilesIndex
  manifest?: DependencyManifest | undefined
}

export type Cafs = {
  cafsDir: string
  addFilesFromDir: (dir: string) => AddToStoreResult
  addFilesFromTarball: (buffer: Buffer) => AddToStoreResult
  getFilePathInCafs: (
    integrity: string | IntegrityLike,
    fileType: FileType
  ) => string
  getFilePathByModeInCafs: (
    integrity: string | IntegrityLike,
    mode: number
  ) => string
  importPackage: ImportPackageFunction
  tempDir: () => Promise<string>
}

export type ParentPackages = Array<{ name?: string | undefined; version?: string | undefined }>

export type MissingPeerDependencyIssue = {
  parents: ParentPackages
  optional: boolean
  wantedRange: string
}

export type MissingPeerIssuesByPeerName = Record<
  string,
  MissingPeerDependencyIssue[]
>

export type BadPeerDependencyIssue = MissingPeerDependencyIssue & {
  foundVersion?: string | undefined
  resolvedFrom: ParentPackages
}

export type BadPeerIssuesByPeerName = Record<string, BadPeerDependencyIssue[]>

export type PeerDependencyIssuesByProjects = Record<
  string,
  PeerDependencyIssues
>

// child nodeId by child alias name in case of non-linked deps
export type ChildrenMap = {
  [alias: string]: string
}

export type DependenciesTreeNode<T> = {
  children: (() => ChildrenMap) | ChildrenMap
  installable?: boolean | undefined
} & (
  | {
    resolvedPackage: T & { name?: string | undefined; version?: string | undefined }
    depth: number
  }
  | {
    resolvedPackage: { name?: string | undefined; version?: string | undefined }
    depth: number
  }
)

export type DependenciesTree<T> = Map<
  // a node ID is the join of the package's keypath with a colon
  // E.g., a subdeps node ID which parent is `foo` will be
  // registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
  string,
  DependenciesTreeNode<T>
>

export type ResolvedPackagesByDepPath = Record<string, DependenciesGraphNode>

export type LinkedDependency = {
  isLinkedDependency: true
  optional?: boolean | undefined
  depPath: string
  dev?: boolean | undefined
  resolution?: DirectoryResolution | undefined
  pkgId: string
  version?: string | undefined
  name?: string | undefined
  normalizedPref?: string | undefined
  alias: string
}

export type PendingNode = {
  alias: string
  nodeId: string
  resolvedPackage: DependenciesGraphNode
  depth: number
  installable: boolean
}

export type ChildrenByParentDepPath = {
  [depPath: string]: Array<{
    alias: string
    depPath: string
  }>
}

export type ResolutionContext = {
  autoInstallPeers: boolean
  autoInstallPeersFromHighestMatch: boolean
  allowBuild?: ((pkgName: string) => boolean) | undefined
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  appliedPatches: Set<string>
  updatedSet: Set<string>
  defaultTag: string
  dryRun: boolean
  forceFullResolution: boolean
  ignoreScripts?: boolean | undefined
  resolvedPackagesByDepPath: ResolvedPackagesByDepPath
  outdatedDependencies: { [pkgId: string]: string }
  childrenByParentDepPath: ChildrenByParentDepPath
  patchedDependencies?: Record<string, PatchFile> | undefined
  pendingNodes: PendingNode[]
  wantedLockfile: Lockfile
  currentLockfile: Lockfile
  linkWorkspacePackagesDepth: number
  lockfileDir: string
  storeController: StoreController
  // the IDs of packages that are not installable
  skipped: Set<string>
  dependenciesTree: DependenciesTree<DependenciesGraphNode>
  force: boolean
  preferWorkspacePackages?: boolean | undefined
  readPackageHook?: ReadPackageHook | undefined
  engineStrict: boolean
  nodeVersion: string
  pnpmVersion: string
  registries: Registries
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct' | undefined
  virtualStoreDir: string
  workspacePackages?: WorkspacePackages
  missingPeersOfChildrenByPkgId: Record<
    string,
    { parentImporterId: string; missingPeersOfChildren: MissingPeersOfChildren }
  >
}

export type MissingPeers = Record<string, string>

export type ResolvedPeers = Record<string, PkgAddress>

export type MissingPeersOfChildren = {
  resolve: (missingPeers: MissingPeers) => void
  reject: (err: Error) => void
  get: () => Promise<MissingPeers>
  resolved?: boolean | undefined
}

export type PkgAddress = {
  name?: string | undefined
  alias: string
  depIsLinked?: boolean | undefined
  depPath: string
  isNew?: boolean | undefined
  isLinkedDependency?: boolean | undefined
  nodeId?: string | undefined
  pkgId: string
  normalizedPref?: string | undefined // is returned only for root dependencies
  installable?: boolean | undefined
  pkg?: PackageManifest | ProjectManifest | undefined
  version?: string | undefined
  updated?: boolean | undefined
  rootDir?: string | undefined
  resolution?: DirectoryResolution | undefined
  dev?: boolean | undefined
  missingPeers?: MissingPeers | undefined
  missingPeersOfChildren?: MissingPeersOfChildren | undefined
  publishedAt?: string | undefined
  optional?: boolean | undefined
} & (
  | {
    isLinkedDependency: true
    version: string
  }
  | {
    isLinkedDependency: undefined
  }
)

export type PeerDependency = {
  version: string
  optional?: boolean | undefined
}

export type PeerDependencies = Record<string, PeerDependency>

export type ParentPkg = Pick<
  PkgAddress,
  'nodeId' | 'installable' | 'depPath' | 'rootDir' | 'optional'
>

export type ParentPkgAliases = Record<string, PkgAddress | true>

export type UpdateMatchingFunction = (pkgName: string) => boolean

export type ResolvedDependenciesOptions = {
  currentDepth: number
  parentPkg: ParentPkg
  parentPkgAliases: ParentPkgAliases
  // If the package has been updated, the dependencies
  // which were used by the previous version are passed
  // via this option
  preferredDependencies?: ResolvedDependencies | undefined
  proceed: boolean
  publishedBy?: Date | undefined
  pickLowestVersion?: boolean | undefined
  resolvedDependencies?: ResolvedDependencies | undefined
  updateMatching?: UpdateMatchingFunction | undefined
  updateDepth: number
  prefix: string
  supportedArchitectures?: SupportedArchitectures | undefined
  updateToLatest?: boolean | undefined
}

export type PostponedResolutionOpts = {
  preferredVersions: PreferredVersions
  parentPkgAliases: ParentPkgAliases
  publishedBy?: Date | undefined
}

export type PeersResolutionResult = {
  missingPeers: MissingPeers | undefined
  resolvedPeers: ResolvedPeers
}

export type PostponedResolutionFunction = (
  opts: PostponedResolutionOpts
) => Promise<PeersResolutionResult>

export type PostponedPeersResolutionFunction = (
  parentPkgAliases: ParentPkgAliases
) => Promise<PeersResolutionResult>

export type ResolvedRootDependenciesResult = {
  pkgAddressesByImporters: Array<Array<PkgAddress | LinkedDependency>>
  time?: Record<string, string> | undefined
}

export type HoistingLimits = Map<string, Set<string>>

export type ReporterFunction = (logObj: LogBase) => void

export type ProjectOptions = {
  buildIndex?: number | undefined
  binsDir?: string | undefined
  manifest?: ProjectManifest | undefined
  modulesDir?: string | undefined
  rootDir: string
}

export type HookOptions = {
  originalManifest?: ProjectManifest | undefined
}

export type FetchOptions = {
  filesIndexFile: string
  lockfileDir: string
  onStart?: ((totalSize: number | null, attempt: number) => void) | undefined
  onProgress?: ((downloaded: number) => void | undefined) | undefined
  readManifest?: boolean | undefined
  pkg: PkgNameVersion
}

export type ResolveDependencyOptions = {
  currentDepth: number
  currentPkg?: {
    depPath?: string | undefined
    name?: string | undefined
    version?: string | undefined
    pkgId?: string | undefined
    resolution?: Resolution | undefined
    dependencyLockfile?: PackageSnapshot | undefined
  } | undefined
  parentPkg: ParentPkg
  parentPkgAliases: ParentPkgAliases
  preferredVersions: PreferredVersions
  prefix: string
  proceed: boolean
  publishedBy?: Date | undefined
  pickLowestVersion?: boolean | undefined
  update: boolean
  updateDepth: number
  updateMatching?: UpdateMatchingFunction | undefined
  supportedArchitectures?: SupportedArchitectures | undefined
  updateToLatest?: boolean | undefined
}

export type ResolveDependencyResult = PkgAddress | LinkedDependency | null

export type FetchResult = {
  local?: boolean | undefined
  manifest?: DependencyManifest | undefined
  filesIndex: Record<string, string>
}

export type DirectoryFetcherOptions = {
  lockfileDir: string | undefined
  readManifest?: boolean | undefined
}

export type FetchFunction<
  FetcherResolution = Resolution,
  Options = FetchOptions,
  Result = FetchResult,
> = (
  cafs: Cafs,
  resolution: FetcherResolution,
  opts: Options
) => Promise<Result>

export type DirectoryFetcherResult = {
  local: true
  filesIndex: Record<string, string>
  packageImportMethod: 'hardlink'
  manifest?: DependencyManifest | undefined
}

export type DirectoryFetcher = FetchFunction<
  DirectoryResolution,
  DirectoryFetcherOptions,
  DirectoryFetcherResult
>

export type GitFetcherOptions = {
  readManifest?: boolean | undefined
  filesIndexFile: string
  pkg?: PkgNameVersion | undefined
}

export type ResolveResult = {
  id: string
  latest?: string | undefined
  publishedAt?: string | undefined
  manifest?: DependencyManifest | undefined
  normalizedPref?: string | undefined // is null for npm-hosted dependencies
  resolution: Resolution
  resolvedVia:
    | 'npm-registry'
    | 'git-repository'
    | 'local-filesystem'
    | 'url'
    | string
}

export type WorkspacePackages = {
  [name: string]: {
    [version: string]: {
      dir: string
      manifest: DependencyManifest
    }
  }
}

export type GitFetcher = FetchFunction<
  GitResolution,
  GitFetcherOptions,
  { filesIndex: Record<string, string>; manifest?: DependencyManifest | undefined }
>

export type Fetchers = {
  localTarball: FetchFunction
  remoteTarball: FetchFunction
  gitHostedTarball: FetchFunction
  directory: DirectoryFetcher
  git: GitFetcher
}

export type GitResolution = {
  commit: string
  repo: string
  type: 'git'
}

export type ResolveOptions = {
  alwaysTryWorkspacePackages?: boolean | undefined
  defaultTag?: string | undefined
  pickLowestVersion?: boolean | undefined
  publishedBy?: Date | undefined
  projectDir: string
  lockfileDir: string
  preferredVersions: PreferredVersions
  preferWorkspacePackages?: boolean | undefined
  registry: string
  workspacePackages?: WorkspacePackages | undefined
  updateToLatest?: boolean | undefined
}

export type ResolveFunction = (
  wantedDependency: WantedDependency,
  opts: ResolveOptions
) => Promise<ResolveResult>

export type VersionSelectorType = 'version' | 'range' | 'tag'

export type VersionSelectorWithWeight = {
  selectorType: VersionSelectorType
  weight: number
}

export type VersionSelectors = {
  [selector: string]: VersionSelectorWithWeight | VersionSelectorType
}

export type PreferredVersions = {
  [packageName: string]: VersionSelectors
}

export type Command = {
  name: string
  path: string
}

export type LockfileV6 = {
  importers: Record<string, ProjectSnapshotV6>
  lockfileVersion: number | string
  time?: Record<string, string> | undefined
  packages?: PackageSnapshots | undefined
  neverBuiltDependencies?: string[] | undefined
  onlyBuiltDependencies?: string[] | undefined
  overrides?: Record<string, string> | undefined
  packageExtensionsChecksum?: string | undefined
  patchedDependencies?: Record<string, PatchFile> | undefined
  settings?: LockfileSettings | undefined
}

export type ProjectSnapshotV6 = {
  specifiers: ResolvedDependenciesOfImporters
  dependencies?: ResolvedDependenciesOfImporters | undefined
  optionalDependencies?: ResolvedDependenciesOfImporters | undefined
  devDependencies?: ResolvedDependenciesOfImporters | undefined
  dependenciesMeta?: DependenciesMeta | undefined
  publishDirectory?: string | undefined
}

export type ResolvedDependenciesOfImporters = Record<
  string,
  { version: string; specifier: string }
>

export type RequestPackageOptions = {
  alwaysTryWorkspacePackages?: boolean | undefined
  currentPkg?: {
    id?: string | undefined
    resolution?: Resolution | undefined
  } | undefined
  /**
   * Expected package is the package name and version that are found in the lockfile.
   */
  expectedPkg?: PkgNameVersion | undefined
  defaultTag?: string | undefined
  pickLowestVersion?: boolean | undefined
  publishedBy?: Date | undefined
  downloadPriority: number
  ignoreScripts?: boolean | undefined
  projectDir?: string | undefined
  lockfileDir: string
  preferredVersions: PreferredVersions
  preferWorkspacePackages?: boolean | undefined
  registry: string
  sideEffectsCache?: boolean | undefined
  skipFetch?: boolean | undefined
  update?: boolean | undefined
  workspacePackages?: WorkspacePackages | undefined
  forceResolve?: boolean | undefined
  supportedArchitectures?: SupportedArchitectures | undefined
  onFetchError?: OnFetchError | undefined
  updateToLatest?: boolean | undefined
}

export type BundledManifest = Pick<
  DependencyManifest,
  | 'bin'
  | 'bundledDependencies'
  | 'bundleDependencies'
  | 'dependencies'
  | 'directories'
  | 'engines'
  | 'name'
  | 'optionalDependencies'
  | 'os'
  | 'peerDependencies'
  | 'peerDependenciesMeta'
  | 'scripts'
  | 'version'
>

export type UploadPkgToStoreOpts = {
  filesIndexFile: string
  sideEffectsCacheKey: string
}

export type UploadPkgToStore = (
  builtPkgLocation: string,
  opts: UploadPkgToStoreOpts
) => Promise<void>

export type PkgRequestFetchResult = {
  bundledManifest?: BundledManifest | undefined
  files: PackageFilesResponse
}

export type FetchPackageToStoreFunction = (
  opts: FetchPackageToStoreOptions
) => {
  filesIndexFile?: string | undefined
  fetching?: () => Promise<PkgRequestFetchResult>
}

export type FetchPackageToStoreFunctionAsync = (
  opts: FetchPackageToStoreOptions
) => Promise<{
  filesIndexFile: string
  fetching: () => Promise<PkgRequestFetchResult>
}>

export type GetFilesIndexFilePath = (
  opts: Pick<FetchPackageToStoreOptions, 'pkg' | 'ignoreScripts'>
) => {
  filesIndexFile: string
  target: string
}

export type FetchPackageToStoreOptions = {
  fetchRawManifest?: boolean | undefined
  force: boolean
  ignoreScripts?: boolean | undefined
  lockfileDir: string
  pkg: PkgNameVersion & {
    id: string
    resolution: Resolution
  }
  /**
   * Expected package is the package name and version that are found in the lockfile.
   */
  expectedPkg?: PkgNameVersion | undefined
  onFetchError?: OnFetchError | undefined
}

export type OnFetchError = (error: Error) => Error

export type BundledManifestFunction = () => Promise<BundledManifest | undefined>

export type PackageResponse = {
  fetching?: (() => Promise<PkgRequestFetchResult>) | undefined
  filesIndexFile?: string | undefined
  body: {
    isLocal: boolean
    isInstallable?: boolean | undefined
    resolution: Resolution
    manifest?: PackageManifest | undefined
    id: string
    normalizedPref?: string | undefined
    updated: boolean
    publishedAt?: string | undefined
    resolvedVia?: string | undefined
    // This is useful for recommending updates.
    // If latest does not equal the version of the
    // resolved package, it is out-of-date.
    latest?: string | undefined
  } & (
    | {
      isLocal: true
      resolution: DirectoryResolution
    }
    | {
      isLocal: false
    }
  )
}

export type FilesMap = Record<string, string>

export type ImportOptions = {
  disableRelinkLocalDirDeps?: boolean | undefined
  filesMap?: FilesMap | undefined
  force: boolean
  resolvedFrom?: ResolvedFrom | undefined
  keepModulesDir?: boolean | undefined
}

export type ImportIndexedPackage = (
  to: string,
  opts: ImportOptions
) => string | undefined

export type ImportIndexedPackageAsync = (
  to: string,
  opts: ImportOptions
) => Promise<string | undefined>

export type RequestPackageFunction = (
  wantedDependency: WantedDependency & { optional?: boolean | undefined },
  options: RequestPackageOptions
) => Promise<PackageResponse>

export type StoreController = {
  requestPackage: RequestPackageFunction
  fetchPackage: FetchPackageToStoreFunction | FetchPackageToStoreFunctionAsync
  getFilesIndexFilePath: GetFilesIndexFilePath
  importPackage: ImportPackageFunctionAsync
  close: () => Promise<void>
  prune: (removeAlienFiles?: boolean | undefined) => Promise<void>
  upload: UploadPkgToStore
}

export type HeadlessOptions = {
  neverBuiltDependencies?: string[] | undefined
  onlyBuiltDependencies?: string[] | undefined
  onlyBuiltDependenciesFile?: string | undefined
  autoInstallPeers?: boolean | undefined
  childConcurrency?: number | undefined
  currentLockfile?: Lockfile | undefined
  currentEngine: {
    nodeVersion: string
    pnpmVersion: string
  }
  dedupeDirectDeps?: boolean | undefined
  enablePnp?: boolean | undefined
  engineStrict: boolean
  excludeLinksFromLockfile?: boolean | undefined
  extraBinPaths?: string[] | undefined
  extraEnv?: Record<string, string> | undefined
  extraNodePaths?: string[] | undefined
  preferSymlinkedExecutables?: boolean | undefined
  hoistingLimits?: HoistingLimits | undefined
  externalDependencies?: Set<string> | undefined
  ignoreDepScripts: boolean
  ignoreScripts: boolean
  ignorePackageManifest?: boolean | undefined
  include: IncludedDependencies
  selectedProjectDirs: string[]
  allProjects: Record<string, { modulesDir: string; id: string; } & HookOptions & ProjectOptions>
  prunedAt?: string | undefined
  hoistedDependencies: HoistedDependencies
  hoistPattern?: string[] | undefined
  publicHoistPattern?: string[] | undefined
  currentHoistedLocations?: Record<string, string[]> | undefined
  lockfileDir: string
  modulesDir?: string | undefined
  virtualStoreDir?: string | undefined
  patchedDependencies?: Record<string, PatchFile> | undefined
  scriptsPrependNodePath?: boolean | 'warn-only' | undefined
  scriptShell?: string | undefined
  shellEmulator?: boolean | undefined
  storeController: StoreController
  sideEffectsCacheRead: boolean
  sideEffectsCacheWrite: boolean
  symlink?: boolean | undefined
  disableRelinkLocalDirDeps?: boolean | undefined
  force: boolean
  storeDir: string
  rawConfig: object
  unsafePerm: boolean
  userAgent: string
  registries: Registries
  reporter?: ReporterFunction | undefined
  packageManager: {
    name: string
    version: string
  }
  pruneStore: boolean
  pruneVirtualStore?: boolean | undefined
  wantedLockfile?: Lockfile | undefined
  ownLifecycleHooksStdio?: 'inherit' | 'pipe' | undefined
  pendingBuilds: string[]
  resolveSymlinksInInjectedDirs?: boolean | undefined
  skipped: Set<string>
  enableModulesDir?: boolean | undefined
  nodeLinker?: 'isolated' | 'hoisted' | 'pnp' | undefined
  useGitBranchLockfile?: boolean | undefined
  useLockfile?: boolean | undefined
  supportedArchitectures?: SupportedArchitectures | undefined
  hoistWorkspacePackages?: boolean | undefined
}

export type InstallationResultStats = {
  added: number
  removed: number
  linkedToRoot: number
}

export type InstallationResult = {
  stats: InstallationResultStats
}

export type PeerDependencyIssues = {
  bad: BadPeerIssuesByPeerName
  missing: MissingPeerIssuesByPeerName
  conflicts: string[]
  intersections: Record<string, string>
}

export type UpdatedProject = {
  originalManifest?: ProjectManifest | undefined
  manifest?: ProjectManifest | undefined
  peerDependencyIssues?: PeerDependencyIssues | undefined
  rootDir: string
}

export type MutateModulesResult = {
  updatedProjects: UpdatedProject[]
  stats: InstallationResultStats
}

export type HoistOpts = GetHoistedDependenciesOpts & {
  virtualStoreDir: string
  extraNodePath?: string[] | undefined
  preferSymlinkedExecutables?: boolean | undefined
}

export type LinkAllBinsOptions = {
  extraNodePaths?: string[]
  hoistedAliasesWithBins: string[]
  preferSymlinkedExecutables?: boolean
}

export type Dependency = {
  children: Record<string, string>
  depPath: string
  depth: number
}

export type HoistGraphResult = {
  hoistedDependencies: HoistedDependencies
  hoistedAliasesWithBins: string[]
}

export type GetAliasHoistType = (alias: string) => 'private' | 'public' | false
