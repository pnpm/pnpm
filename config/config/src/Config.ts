import type { Catalogs } from '@pnpm/catalogs.types'
import {
  type Project,
  type ProjectManifest,
  type ProjectsGraph,
  type Registries,
  type SslConfig,
} from '@pnpm/types'
import type { Hooks } from '@pnpm/pnpmfile'

export type UniversalOptions = Pick<Config, 'color' | 'dir' | 'rawConfig' | 'rawLocalConfig'>

export interface WantedPackageManager {
  name: string
  version?: string
}

export type VerifyDepsBeforeRun = 'install' | 'warn' | 'error' | 'prompt' | false

export interface Config {
  allProjects?: Project[]
  selectedProjectsGraph?: ProjectsGraph
  allProjectsGraph?: ProjectsGraph

  allowNew: boolean
  autoInstallPeers?: boolean
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
  dryRun?: boolean // This option might be not supported ever
  global?: boolean
  dir: string
  bin: string
  verifyDepsBeforeRun?: VerifyDepsBeforeRun
  ignoreDepScripts?: boolean
  ignoreScripts?: boolean
  ignoreCompatibilityDb?: boolean
  includeWorkspaceRoot?: boolean
  optimisticRepeatInstall?: boolean
  save?: boolean
  saveProd?: boolean
  saveDev?: boolean
  saveOptional?: boolean
  savePeer?: boolean
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
  offline?: boolean
  registry?: string
  optional?: boolean
  unsafePerm?: boolean
  loglevel?: 'silent' | 'error' | 'warn' | 'info' | 'debug'
  frozenLockfile?: boolean
  preferFrozenLockfile?: boolean
  only?: 'prod' | 'production' | 'dev' | 'development'
  packageManager: {
    name: string
    version: string
  }
  wantedPackageManager?: WantedPackageManager
  preferOffline?: boolean
  sideEffectsCache?: boolean // for backward compatibility
  sideEffectsCacheReadonly?: boolean // for backward compatibility
  sideEffectsCacheRead?: boolean
  sideEffectsCacheWrite?: boolean
  shamefullyHoist?: boolean
  dev?: boolean
  ignoreCurrentPrefs?: boolean
  recursive?: boolean
  enablePrePostScripts?: boolean
  useNodeVersion?: string
  useStderr?: boolean
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
  preferSymlinkedExecutables?: boolean
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct'
  registrySupportsTimeField?: boolean
  failedToLoadBuiltInConfig: boolean
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
  verifyStoreIntegrity?: boolean
  maxSockets?: number
  networkConcurrency?: number
  fetchingConcurrency?: number
  lockfileOnly?: boolean // like npm's --package-lock-only
  childConcurrency?: number
  ignorePnpmfile?: boolean
  pnpmfile: string
  hooks?: Hooks
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
  onlyBuiltDependencies?: string[]
  dedupePeerDependents?: boolean
  patchesDir?: string
  ignoreWorkspaceCycles?: boolean
  disallowWorkspaceCycles?: boolean
  packGzipLevel?: number

  registries: Registries
  sslConfigs: Record<string, SslConfig>
  ignoreWorkspaceRootCheck: boolean
  workspaceRoot: boolean

  testPattern?: string[]
  changedFilesIgnorePattern?: string[]
  rootProjectManifestDir: string
  rootProjectManifest?: ProjectManifest
  userConfig: Record<string, string>

  globalconfig: string
  hoist: boolean
  packageLock: boolean
  pending: boolean
  userconfig: string
  workspacePrefix?: string
  dedupeDirectDeps?: boolean
  extendNodePath?: boolean
  gitBranchLockfile?: boolean
  globalDir?: string
  globalPkgDir: string
  lockfile?: boolean
  dedupeInjectedDeps?: boolean
  nodeOptions?: string
  packageManagerStrict?: boolean
  packageManagerStrictVersion?: boolean
  virtualStoreDirMaxLength: number
  peersSuffixMaxLength?: number
  strictStorePkgContentCheck: boolean
  managePackageManagerVersions: boolean
  strictDepBuilds: boolean
}

export interface ConfigWithDeprecatedSettings extends Config {
  globalPrefix?: string
  proxy?: string
  shamefullyFlatten?: boolean
}
