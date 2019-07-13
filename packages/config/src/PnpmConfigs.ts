import { Registries } from '@pnpm/types'

export interface PnpmConfigs extends Record<string, any> { // tslint:disable-line
  bail: boolean,
  cliArgs: Record<string, any>, // tslint:disable-line
  cliV4Beta: boolean,
  extraBinPaths: string[],
  filter: string[],
  rawNpmConfig: Record<string, any>, // tslint:disable-line
  globalPrefix: string,
  globalBin: string,
  dryRun?: boolean, // This option might be not supported ever
  global?: boolean,
  prefix: string,
  bin?: string,
  ignoreScripts?: boolean
  save?: boolean,
  saveProd?: boolean,
  saveDev?: boolean,
  saveOptional?: boolean,
  savePeer?: boolean,
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
  store?: string,
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
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'reflink',
  shamefullyFlatten?: boolean,
  useStoreServer?: boolean,
  workspaceConcurrency: number,
  workspacePrefix?: string,
  reporter?: string,
  linkWorkspacePackages: boolean,
  sort: boolean,
  strictPeerDependencies: boolean,
  pending: boolean,
  lockfileDirectory?: string,
  sharedWorkspaceLockfile: boolean,
  useLockfile: boolean,
  resolutionStrategy: 'fast' | 'fewer-dependencies',

  registries: Registries,
}
