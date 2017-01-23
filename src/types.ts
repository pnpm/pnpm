import {ReporterType} from './reporter'
import {PackageSpec, ResolveOptions, ResolveResult, Resolution} from './resolve';
import {FetchOptions} from './install/fetchResolution';

export type LifecycleHooks = {

  /**
   * Executes before the pnpm's default resolution takes action.
   *
   * If this method returns a resolution then it is used, otherwise pnpm's
   * default resolution algorithm will be used.
   */
  packageWillResolve?: (spec: PackageSpec, opts: ResolveOptions) => Promise<ResolveResult | null>,

  /**
   * Executes when package is about to fetch its resolution.
   *
   * This can be used to implement custom fetching logic.
   */
  packageWillFetch?: (target: string, resolution: Resolution, opts: FetchOptions) => Promise<boolean>,

  /**
   * Executes just after the package is fetched into a target directory.
   *
   * This is the opportunity to write into the target directory before it's
   * commited.
   */
  packageDidFetch?: (target: string, resolution: Resolution) => Promise<void>,

  // TODO: add more lifecycle hooks
  //
  // packageDidResolve
  //
  // packageWillInstall
  // packageDidInstall
};

export type PnpmOptions = {
  cwd?: string,
  global?: boolean,
  globalPath?: string,
  storePath?: string,
  cachePath?: string,
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

  lifecycle?: LifecycleHooks,

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

  cacheTTL?: number,
}

export type StrictPnpmOptions = {
  cwd: string,
  global: boolean,
  globalPath: string,
  storePath: string,
  cachePath: string,
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

  lifecycle: LifecycleHooks,

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

  cacheTTL: number,
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
