import {
  Project,
  ProjectManifest,
  ProjectsGraph,
  Registries,
} from '@pnpm/types'
import type { Hooks } from '@pnpm/pnpmfile'

export type UniversalOptions = Pick<Config, 'color' | 'dir' | 'rawConfig' | 'rawLocalConfig'>

export interface Config {
  allProjects?: Project[]
  selectedProjectsGraph?: ProjectsGraph

  allowNew: boolean
  autoInstallPeers?: boolean
  bail: boolean
  color: 'always' | 'auto' | 'never'
  cliOptions: Record<string, any>, // eslint-disable-line
  useBetaCli: boolean
  extraBinPaths: string[]
  filter: string[]
  filterProd: string[]
  rawLocalConfig: Record<string, any>, // eslint-disable-line
  rawConfig: Record<string, any>, // eslint-disable-line
  dryRun?: boolean // This option might be not supported ever
  global?: boolean
  dir: string
  bin: string
  ignoreScripts?: boolean
  includeWorkspaceRoot?: boolean
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

  alwaysAuth?: boolean

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
  repeatInstallDepth?: number
  ignorePnpmfile?: boolean
  pnpmfile: string
  hooks?: Hooks
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  hoistPattern?: string[]
  publicHoistPattern?: string[]
  useStoreServer?: boolean
  useRunningStoreServer?: boolean
  workspaceConcurrency: number
  workspaceDir?: string
  reporter?: string
  aggregateOutput: boolean
  linkWorkspacePackages: boolean | 'deep'
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
  embedReadme?: boolean
  gitShallowHosts?: string[]
  legacyDirFiltering?: boolean

  registries: Registries
  ignoreWorkspaceRootCheck: boolean
  workspaceRoot: boolean

  testPattern?: string[]
  changedFilesIgnorePattern?: string[]
  rootProjectManifest?: ProjectManifest
  userConfig: Record<string, string>

  // feature flags for experimental testing
  useInlineSpecifiersLockfileFormat?: boolean // For https://github.com/pnpm/pnpm/issues/4725
}

export interface ConfigWithDeprecatedSettings extends Config {
  globalPrefix?: string
  proxy?: string
}
