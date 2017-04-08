import {ReporterType} from './reporter'
import {PackageMeta} from './resolve'

export type PnpmOptions = {
  cwd?: string,
  global?: boolean,
  globalPath?: string,
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
  exit0?: boolean,

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
}

export type StrictPnpmOptions = {
  cwd: string,
  global: boolean,
  globalPath: string,
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
  exit0: boolean,

  // proxy
  proxy?: string,
  httpsProxy?: string,
  localAddress?: string,

  // ssl
  cert?: string,
  key?: string,
  ca?: string,
  strictSsl: boolean,

  userAgent?: string,
  tag: string,

  metaCache: Map<string, PackageMeta>,
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
