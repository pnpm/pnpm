import {ReporterType} from './reporter'
import {PackageMeta} from './resolve'

export type PnpmOptions = {
  rawNpmConfig?: Object,
  global?: boolean,
  prefix?: string,
  bin?: string,
  storePath?: string,
  localRegistry?: string,
  ignoreScripts?: boolean
  save?: boolean,
  saveDev?: boolean,
  saveOptional?: boolean,
  production?: boolean,
  fetchRetries?: number,
  fetchRetryFactor?: number,
  fetchRetryMintimeout?: number,
  fetchRetryMaxtimeout?: number,
  saveExact?: boolean,
  force?: boolean,
  depth?: number,
  engineStrict?: boolean,
  nodeVersion?: string,
  networkConcurrency?: number,
  fetchingConcurrency?: number,
  childConcurrency?: number,
  lockStaleDuration?: number,
  offline?: boolean,
  registry?: string,

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

  // cannot be specified via configs
  update?: boolean,
}

export type StrictPnpmOptions = {
  rawNpmConfig: Object,
  global: boolean,
  prefix: string,
  bin: string,
  storePath: string,
  localRegistry: string,
  ignoreScripts: boolean
  save: boolean,
  saveDev: boolean,
  saveOptional: boolean,
  production: boolean,
  fetchRetries: number,
  fetchRetryFactor: number,
  fetchRetryMintimeout: number,
  fetchRetryMaxtimeout: number,
  saveExact: boolean,
  force: boolean,
  depth: number,
  engineStrict: boolean,
  nodeVersion: string,
  networkConcurrency: number,
  fetchingConcurrency: number,
  lockStaleDuration: number,
  childConcurrency: number,
  offline: boolean,
  registry: string,

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

  // cannot be specified via configs
  update: boolean,
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
}
