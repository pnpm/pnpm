import {
  Project,
  ProjectsGraph,
  Registries,
} from '@pnpm/types'
import type { Hooks } from '@pnpm/pnpmfile'

export type UniversalOptions = Pick<Config, 'color' | 'dir' | 'rawConfig' | 'rawLocalConfig'>

export interface Config {
  allProjects?: Project[]
  selectedProjectsGraph?: ProjectsGraph

  allowNew: boolean
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
  save?: boolean
  saveProd?: boolean
  saveDev?: boolean
  saveOptional?: boolean
  savePeer?: boolean
  saveWorkspaceProtocol?: boolean
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
  sideEffectsCache?: boolean
  sideEffectsCacheReadonly?: boolean
  shamefullyHoist?: boolean
  dev?: boolean
  ignoreCurrentPrefs?: boolean
  recursive?: boolean
  enablePrePostScripts?: boolean
  useNodeVersion?: string
  useStderr?: boolean

  // proxy
  httpProxy?: string
  httpsProxy?: string
  localAddress?: string
  noProxy?: string | boolean

  // ssl
  cert?: string
  key?: string
  ca?: string
  strictSsl?: boolean

  userAgent?: string
  tag?: string

  alwaysAuth?: boolean

  // pnpm specific configs
  cacheDir: string
  stateDir: string
  storeDir?: string
  virtualStoreDir?: string
  verifyStoreIntegrity?: boolean
  networkConcurrency?: number
  fetchingConcurrency?: number
  lockfileOnly?: boolean // like npm's --package-lock-only
  childConcurrency?: number
  repeatInstallDepth?: number
  ignorePnpmfile?: boolean
  pnpmfile: string
  hooks?: Hooks
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone'
  hoistPattern?: string[]
  publicHoistPattern?: string[]
  useStoreServer?: boolean
  useRunningStoreServer?: boolean
  workspaceConcurrency: number
  workspaceDir?: string
  reporter?: string
  linkWorkspacePackages: boolean | 'deep'
  preferWorkspacePackages: boolean
  reverse: boolean
  sort: boolean
  strictPeerDependencies: boolean
  lockfileDir?: string
  modulesDir?: string
  sharedWorkspaceLockfile?: boolean
  useLockfile: boolean
  globalPnpmfile?: string
  npmPath?: string
  gitChecks?: boolean
  publishBranch?: string
  recursiveInstall?: boolean
  symlink: boolean
  enablePnp?: boolean
  enableModulesDir: boolean
  modulesCacheMaxAge: number

  registries: Registries
  ignoreWorkspaceRootCheck: boolean
  workspaceRoot: boolean

  testPattern?: string[]
  changedFilesIgnorePattern?: string[]
  extendNodePath?: boolean
}

export interface ConfigWithDeprecatedSettings extends Config {
  frozenShrinkwrap?: boolean
  globalPrefix?: string
  proxy?: string
  lockfileDirectory?: string
  preferFrozenShrinkwrap?: boolean
  sharedWorkspaceShrinkwrap?: boolean
  shrinkwrapDirectory?: string
  shrinkwrapOnly?: boolean
}
