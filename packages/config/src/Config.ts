import { ImporterManifest, IncludedDependencies, Registries } from '@pnpm/types'

export type UniversalOptions = Pick<Config, 'color' | 'dir' | 'rawConfig' | 'rawLocalConfig'>

export type WsPkg = {
  dir: string,
  manifest: ImporterManifest,
  writeImporterManifest: (manifest: ImporterManifest) => Promise<void>,
}

export type WsPkgsGraph = Record<string, { dependencies: string[], package: WsPkg }>

export interface Config {
  allWsPkgs?: WsPkg[],
  selectedWsPkgsGraph?: WsPkgsGraph,

  allowNew: boolean,
  bail: boolean,
  color: 'always' | 'auto' | 'never',
  cliOptions: Record<string, any>, // tslint:disable-line
  useBetaCli: boolean,
  extraBinPaths: string[],
  filter: string[],
  rawLocalConfig: Record<string, any>, // tslint:disable-line
  rawConfig: Record<string, any>, // tslint:disable-line
  globalBin: string,
  dryRun?: boolean, // This option might be not supported ever
  global?: boolean,
  globalDir: string,
  dir: string,
  bin?: string,
  ignoreScripts?: boolean
  include: IncludedDependencies
  save?: boolean,
  saveProd?: boolean,
  saveDev?: boolean,
  saveOptional?: boolean,
  savePeer?: boolean,
  saveWorkspaceProtocol?: boolean,
  production?: boolean,
  development?: boolean,
  fetchRetries?: number,
  fetchRetryFactor?: number,
  fetchRetryMintimeout?: number,
  fetchRetryMaxtimeout?: number,
  saveExact?: boolean,
  savePrefix?: string,
  force?: boolean,
  depth?: number,
  engineStrict?: boolean,
  nodeVersion?: string,
  offline?: boolean,
  registry?: string,
  optional?: boolean,
  unsafePerm?: boolean,
  loglevel?: 'silent' | 'error' | 'warn' | 'notice' | 'http' | 'timing' | 'info' | 'verbose' | 'silly',
  frozenLockfile?: boolean,
  preferFrozenLockfile?: boolean,
  only?: 'prod' | 'production' | 'dev' | 'development',
  packageManager: {
    name: string,
    version: string,
  },
  sideEffectsCache?: boolean,
  sideEffectsCacheRead?: boolean,
  sideEffectsCacheWrite?: boolean,
  sideEffectsCacheReadonly?: boolean,
  shamefullyHoist?: boolean,
  dev?: boolean,
  ignoreCurrentPrefs?: boolean,

  // proxy
  proxy?: string,
  httpsProxy?: string,
  localAddress?: string,

  // ssl
  cert?: string,
  key?: string,
  ca?: string,
  strictSsl?: boolean,

  userAgent?: string,
  tag?: string,

  alwaysAuth?: boolean,

  // pnpm specific configs
  storeDir?: string,
  virtualStoreDir?: string,
  verifyStoreIntegrity?: boolean,
  networkConcurrency?: number,
  fetchingConcurrency?: number,
  lockStaleDuration?: number,
  lock: boolean,
  lockfileOnly?: boolean, // like npm's --package-lock-only
  childConcurrency?: number,
  repeatInstallDepth?: number,
  ignorePnpmfile?: boolean,
  pnpmfile: string,
  independentLeaves?: boolean,
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone',
  hoistPattern?: string[],
  useStoreServer?: boolean,
  useRunningStoreServer?: boolean,
  workspaceConcurrency: number,
  workspaceDir?: string,
  reporter?: string,
  linkWorkspacePackages: boolean,
  sort: boolean,
  strictPeerDependencies: boolean,
  lockfileDir?: string,
  sharedWorkspaceLockfile?: boolean,
  useLockfile: boolean,
  resolutionStrategy: 'fast' | 'fewer-dependencies',
  globalPnpmfile?: string,

  registries: Registries,
  ignoreWorkspaceRootCheck: boolean,
}

export interface ConfigWithDeprecatedSettings extends Config {
  frozenShrinkwrap?: boolean,
  globalPrefix?: string,
  lockfileDirectory?: string,
  shrinkwrapDirectory?: string,
  shrinkwrapOnly?: boolean,
  preferFrozenShrinkwrap?: boolean,
  sharedWorkspaceShrinkwrap?: boolean,
}
