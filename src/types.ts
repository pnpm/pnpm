import {ReporterType} from './reporter'
import {PackageMeta} from './resolve/utils/loadPackageMeta'

export type PnpmOptions = {
  cwd?: string,
  global?: boolean,
  globalPath?: string,
  storePath?: string,
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
  linkLocal?: boolean,
  depth?: number,
  engineStrict?: boolean,
  nodeVersion?: string,

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
  linkLocal: boolean,
  depth: number,
  engineStrict: boolean,
  nodeVersion: string,

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

export type Package = {
  name: string,
  version: string,
  private?: boolean,
  bin?: string | {
    [name: string]: string
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
