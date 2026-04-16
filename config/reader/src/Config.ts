import type { Catalogs } from '@pnpm/catalogs.types'
import type { Hooks } from '@pnpm/hooks.pnpmfile'
import type {
  EngineDependency,
  Finder,
  Project,
  ProjectManifest,
  ProjectsGraph,
  Registries,
  RegistryConfig,
  TrustPolicy,
} from '@pnpm/types'

import type { OptionsFromRootManifest } from './getOptionsFromRootManifest.js'

export type UniversalOptions = Pick<Config, 'color' | 'dir' | 'authConfig'>


export type VerifyDepsBeforeRun = 'install' | 'warn' | 'error' | 'prompt' | false

/**
 * Runtime state, workspace context, and CLI metadata.
 * These fields are NOT user-facing settings — they are computed at startup
 * or populated later by the CLI harness (e.g. workspace filtering, hook loading).
 */
export interface ConfigContext {
  // -- Runtime state --
  hooks?: Hooks
  finders?: Record<string, Finder>

  // -- Workspace context --
  allProjects?: Project[]
  selectedProjectsGraph?: ProjectsGraph
  allProjectsGraph?: ProjectsGraph
  rootProjectManifest?: ProjectManifest
  rootProjectManifestDir: string

  // -- CLI metadata --
  cliOptions: Record<string, any> // eslint-disable-line
  /** Keys explicitly set from workspace yaml, CLI, or env vars (not defaults). */
  explicitlySetKeys: Set<string>
  packageManager: {
    name: string
    version: string
  }
  wantedPackageManager?: EngineDependency
}

/**
 * User-facing settings + auth/network config.
 * Does NOT include runtime state — see {@link ConfigContext} for that.
 */
export interface Config extends OptionsFromRootManifest {
  allowNew: boolean
  autoConfirmAllPrompts?: boolean
  autoInstallPeers?: boolean
  bail: boolean
  color: 'always' | 'auto' | 'never'
  useBetaCli: boolean
  excludeLinksFromLockfile: boolean
  extraBinPaths: string[]
  extraEnv: Record<string, string>
  failIfNoMatch: boolean
  filter: string[]
  filterProd: string[]
  authConfig: Record<string, any>, // eslint-disable-line
  dryRun?: boolean // This option might be not supported ever
  global?: boolean
  dir: string
  bin: string
  verifyDepsBeforeRun?: VerifyDepsBeforeRun
  ignoreScripts?: boolean
  ignoreCompatibilityDb?: boolean
  includeWorkspaceRoot?: boolean
  optimisticRepeatInstall?: boolean
  save?: boolean
  saveProd?: boolean
  saveDev?: boolean
  saveOptional?: boolean
  savePeer?: boolean
  saveCatalogName?: string
  saveWorkspaceProtocol?: boolean | 'rolling'
  lockfileIncludeTarballUrl?: boolean
  scriptShell?: string
  stream?: boolean
  pnpmExecPath: string
  pnpmHomeDir: string
  production?: boolean
  fetchRetries?: number
  fetchRetryFactor?: number
  fetchRetryMintimeout?: number
  fetchRetryMaxtimeout?: number
  fetchTimeout?: number
  saveExact?: boolean
  savePrefix?: string
  shellEmulator?: boolean
  scriptsPrependNodePath?: boolean | 'warn-only'
  force?: boolean
  depth?: number
  engineStrict?: boolean
  nodeVersion?: string
  nodeDownloadMirrors?: Record<string, string>
  offline?: boolean
  registry?: string
  optional?: boolean
  unsafePerm?: boolean
  loglevel?: 'silent' | 'error' | 'warn' | 'info' | 'debug'
  frozenLockfile?: boolean
  preferFrozenLockfile?: boolean
  only?: 'prod' | 'production' | 'dev' | 'development'
  preferOffline?: boolean
  sideEffectsCache?: boolean // for backward compatibility
  sideEffectsCacheReadonly?: boolean // for backward compatibility
  sideEffectsCacheRead?: boolean
  sideEffectsCacheWrite?: boolean
  shamefullyHoist?: boolean
  dev?: boolean
  ignoreCurrentSpecifiers?: boolean
  recursive?: boolean
  enablePrePostScripts?: boolean
  useStderr?: boolean
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
  preferSymlinkedExecutables?: boolean
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct'
  registrySupportsTimeField?: boolean
  resolvePeersFromWorkspaceRoot?: boolean
  deployAllFiles?: boolean
  forceLegacyDeploy?: boolean
  reporterHidePrefix?: boolean

  // proxy
  httpProxy?: string
  httpsProxy?: string
  localAddress?: string
  noProxy?: string | boolean

  // ssl
  cert?: string | string[]
  key?: string
  ca?: string | string[]
  strictSsl?: boolean

  userAgent?: string
  tag?: string
  updateNotifier?: boolean

  // pnpm specific configs
  cacheDir: string
  configDir: string
  stateDir: string
  storeDir?: string
  virtualStoreDir?: string
  virtualStoreOnly?: boolean
  enableGlobalVirtualStore?: boolean
  verifyStoreIntegrity?: boolean
  maxSockets?: number
  networkConcurrency?: number
  fetchingConcurrency?: number
  lockfileOnly?: boolean // like npm's --package-lock-only
  childConcurrency?: number
  ignorePnpmfile?: boolean
  pnpmfile: string[] | string
  tryLoadDefaultPnpmfile?: boolean
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  hoistPattern?: string[]
  publicHoistPattern?: string[] | string
  hoistWorkspacePackages?: boolean
  useStoreServer?: boolean
  useRunningStoreServer?: boolean
  workspaceConcurrency: number
  workspaceDir?: string
  workspacePackagePatterns?: string[]
  catalogs?: Catalogs
  catalogMode?: 'strict' | 'prefer' | 'manual'
  cleanupUnusedCatalogs?: boolean
  reporter?: string
  aggregateOutput: boolean
  linkWorkspacePackages: boolean | 'deep'
  injectWorkspacePackages?: boolean
  preferWorkspacePackages: boolean
  reverse: boolean
  sort: boolean
  strictPeerDependencies: boolean
  lockfileDir?: string
  modulesDir?: string
  sharedWorkspaceLockfile?: boolean
  useLockfile: boolean
  useGitBranchLockfile: boolean
  mergeGitBranchLockfiles?: boolean
  mergeGitBranchLockfilesBranchPattern?: string[]
  globalPnpmfile?: string
  npmPath?: string
  gitChecks?: boolean
  publishBranch?: string
  recursiveInstall?: boolean
  symlink: boolean
  enablePnp?: boolean
  enableModulesDir: boolean
  modulesCacheMaxAge: number
  dlxCacheMaxAge: number
  embedReadme?: boolean
  gitShallowHosts?: string[]
  legacyDirFiltering?: boolean
  allowBuilds?: Record<string, boolean | string>
  dedupePeerDependents?: boolean
  dedupePeers?: boolean
  patchesDir?: string
  ignoreWorkspaceCycles?: boolean
  disallowWorkspaceCycles?: boolean
  packGzipLevel?: number
  blockExoticSubdeps?: boolean

  registries: Registries
  configByUri: Record<string, RegistryConfig>
  ignoreWorkspaceRootCheck: boolean
  workspaceRoot: boolean

  testPattern?: string[]
  changedFilesIgnorePattern?: string[]
  userConfig: Record<string, string>

  hoist: boolean
  packageLock: boolean
  pending: boolean
  userconfig: string
  npmrcAuthFile?: string
  workspacePrefix?: string
  dedupeDirectDeps?: boolean
  extendNodePath?: boolean
  gitBranchLockfile?: boolean
  globalBinDir?: string
  globalDir?: string
  globalPkgDir: string
  lockfile?: boolean
  dedupeInjectedDeps?: boolean
  nodeOptions?: string
  pmOnFail?: 'download' | 'error' | 'warn' | 'ignore'
  packageManagerStrict?: boolean
  packageManagerStrictVersion?: boolean
  virtualStoreDirMaxLength: number
  peersSuffixMaxLength?: number
  strictStorePkgContentCheck: boolean
  managePackageManagerVersions: boolean
  strictDepBuilds: boolean
  syncInjectedDepsAfterScripts?: string[]
  initPackageManager: boolean
  initType: 'commonjs' | 'module'
  dangerouslyAllowAllBuilds: boolean
  ci: boolean
  preserveAbsolutePaths?: boolean
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
  minimumReleaseAgeStrict?: boolean
  fetchWarnTimeoutMs?: number
  fetchMinSpeedKiBps?: number
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: string[]
  trustPolicyIgnoreAfter?: number
  auditLevel?: 'info' | 'low' | 'moderate' | 'high' | 'critical'

  packageConfigs?: ProjectConfigSet
}

export interface ConfigWithDeprecatedSettings extends Config {
  globalPrefix?: string
  proxy?: string
}

export const PROJECT_CONFIG_FIELDS = [
  'hoist',
  'modulesDir',
  'overrides',
  'saveExact',
  'savePrefix',
] as const satisfies Array<keyof Config>

export type ProjectConfig = Partial<Pick<Config, typeof PROJECT_CONFIG_FIELDS[number] | 'hoistPattern'>>

/** Simple map from project names to {@link ProjectConfig} */
export type ProjectConfigRecord = Record<string, ProjectConfig>

/** Map multiple project names to a shared {@link ProjectConfig} */
export type ProjectConfigMultiMatch = { match: string[] } & ProjectConfig

export type ProjectConfigSet =
  | ProjectConfigRecord
  | ProjectConfigMultiMatch[]
