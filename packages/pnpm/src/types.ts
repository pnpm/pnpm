import {
  IncludedDependencies,
  LogBase,
  PackageManifest,
  Registries,
} from '@pnpm/types'

export type ReadPackageHook = (pkg: PackageManifest) => PackageManifest

export interface PnpmOptions {
  argv: {
    cooked: string[],
    original: string[],
    remain: string[],
  },
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
  savePeer?: boolean,
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
  registries?: Registries,
  registry?: string,
  optional?: boolean,
  unsafePerm?: boolean,
  include: IncludedDependencies,

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

  metaCache?: Map<string, object>,
  alwaysAuth?: boolean,

  // pnpm specific configs
  storePath?: string, // DEPRECATED! store should be used
  store?: string,
  verifyStoreIntegrity?: boolean,
  networkConcurrency?: number,
  fetchingConcurrency?: number,
  lockfileOnly?: boolean, // like npm's --package-lock-only
  lockStaleDuration?: number,
  lock: boolean,
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
  linkWorkspacePackages: boolean,
  sort: boolean,
  pending: boolean,
  lockfileDirectory?: string,

  // cannot be specified via configs
  latest?: boolean,
  update?: boolean,
  reporter?: (logObj: LogBase) => void,
  packageManager?: {
    name: string,
    version: string,
  },

  hooks?: {
    readPackage?: ReadPackageHook,
  },

  ignoreFile?: (filename: string) => boolean,
}
