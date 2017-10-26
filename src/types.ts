import {PackageMeta} from 'package-store'
import {Log} from 'pnpm-logger'

export type PnpmOptions = {
  rawNpmConfig?: Object,
  global?: boolean,
  prefix?: string,
  bin?: string,
  ignoreScripts?: boolean
  save?: boolean,
  saveProd?: boolean,
  saveDev?: boolean,
  saveOptional?: boolean,
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

  metaCache?: Map<string, PackageMeta>,
  alwaysAuth?: boolean,

  // pnpm specific configs
  storePath?: string, // DEPRECATED! store should be used
  store?: string,
  verifyStoreIntegrity?: boolean,
  networkConcurrency?: number,
  fetchingConcurrency?: number,
  lockStaleDuration?: number,
  lock?: boolean,
  childConcurrency?: number,
  repeatInstallDepth?: number,
  independentLeaves?: boolean,

  // cannot be specified via configs
  update?: boolean,
  reporter?: (logObj: Log) => void,
  packageManager?: {
    name: string,
    version: string,
  },

  hooks?: {
    readPackage?: ReadPackageHook,
  },
}

export type ReadPackageHook = (pkg: Package) => Package

export type StrictPnpmOptions = PnpmOptions & {
  rawNpmConfig: Object,
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

  metaCache: Map<string, PackageMeta>,
  alwaysAuth: boolean,

  // pnpm specific configs
  store: string,
  verifyStoreIntegrity: boolean,
  networkConcurrency: number,
  fetchingConcurrency: number,
  lockStaleDuration: number,
  lock: boolean,
  childConcurrency: number,
  repeatInstallDepth: number,
  independentLeaves: boolean,
  locks: string,

  // cannot be specified via configs
  update: boolean,
  packageManager: {
    name: string,
    version: string,
  },

  hooks: {
    readPackage?: ReadPackageHook,
  },
}

export type Dependencies = {
  [name: string]: string
}

export type PackageBin = string | {[name: string]: string}

export type Package = {
  name: string,
  version: string,
  private?: boolean,
  bin?: PackageBin,
  directories?: {
    bin?: string,
  },
  dependencies?: Dependencies,
  devDependencies?: Dependencies,
  optionalDependencies?: Dependencies,
  peerDependencies?: Dependencies,
  bundleDependencies?: string[],
  bundledDependencies?: string[],
  scripts?: {
    [name: string]: string
  },
  config?: Object,
  engines?: {
    node?: string,
    npm?: string,
  },
  cpu?: string[],
  os?: string[],
  // TODO: create a separate type called PackageManifest that will have `deprecated`
  // and won't have `scripts`, `config` and other fields that are not returned by
  // the registry
  deprecated?: string,
}
