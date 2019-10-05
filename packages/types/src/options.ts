import { DependenciesField } from './misc'
import { ImporterManifest, PackageManifest } from './package'

export type LogBase = {
  level: 'debug' | 'error';
} | {
  level: 'info' | 'warn';
  prefix: string;
  message: string;
}

export type IncludedDependencies = {
  [dependenciesField in DependenciesField]: boolean
}

export interface ReadPackageHook {
  (pkg: PackageManifest): PackageManifest
  (pkg: ImporterManifest): ImporterManifest
}

export type StrictPnpmOptions = {
  rawConfig: object,
  dryRun: boolean, // This option might be not supported ever
  global: boolean,
  prefix: string,
  bin: string,
  ignoreScripts: boolean
  save: boolean,
  saveProd: boolean,
  saveDev: boolean,
  saveOptional: boolean,
  production: boolean,
  development: boolean,
  fetchRetries: number,
  fetchRetryFactor: number,
  fetchRetryMintimeout: number,
  fetchRetryMaxtimeout: number,
  saveExact: boolean,
  savePrefix: string,
  force: boolean,
  depth: number,
  engineStrict: boolean,
  nodeVersion: string,
  offline: boolean,
  registry: string,
  optional: boolean,
  unsafePerm: boolean,

  // proxy
  proxy?: string,
  httpsProxy?: string,
  localAddress?: string,

  // ssl
  cert?: string,
  key?: string,
  ca?: string,
  strictSsl: boolean,

  userAgent: string,
  tag: string,

  metaCache: Map<string, object>,
  alwaysAuth: boolean,

  // pnpm specific config
  store: string,
  verifyStoreIntegrity: boolean,
  networkConcurrency: number,
  fetchingConcurrency: number,
  lockfileOnly: boolean, // like npm's --package-lock-only
  lockStaleDuration: number,
  lock: boolean,
  childConcurrency: number,
  repeatInstallDepth: number,
  ignorePnpmfile: boolean,
  independentLeaves: boolean,
  locks: string,
  packageImportMethod: 'auto' | 'hardlink' | 'copy' | 'clone',

  // cannot be specified via config
  update: boolean,
  packageManager: {
    name: string,
    version: string,
  },

  hooks: {
    readPackage?: ReadPackageHook,
  },
}

export type PnpmOptions = Partial<StrictPnpmOptions>
