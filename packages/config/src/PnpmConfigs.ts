export interface PnpmConfigs {
  bail: boolean,
  cliArgs: object,
  filter: string[],
  rawNpmConfig: object,
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
  scope: string, // TODO: deprecate this flag
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
  childConcurrency?: number,
  repeatInstallDepth?: number,
  ignorePnpmfile?: boolean,
  pnpmfile: string,
  independentLeaves?: boolean,
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'reflink',
  shamefullyFlatten?: boolean,
  shrinkwrapOnly?: boolean, // like npm's --package-lock-only
  useStoreServer?: boolean,
  workspaceConcurrency: number,
  workspacePrefix?: string,
  reporter?: string,
}
