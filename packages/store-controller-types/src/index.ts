import {
  DirectoryResolution,
  LocalPackages,
  Resolution,
  WantedDependency,
} from '@pnpm/resolver-base'
import {
  PackageJson,
  PackageManifest,
} from '@pnpm/types'

export * from '@pnpm/resolver-base'

export interface StoreController {
  requestPackage: RequestPackageFunction,
  fetchPackage: FetchPackageToStoreFunction,
  importPackage: ImportPackageFunction,
  close (): Promise<void>,
  updateConnections (prefix: string, opts: {addDependencies: string[], removeDependencies: string[], prune: boolean}): Promise<void>,
  prune (): Promise<void>,
  saveState (): Promise<void>,
  upload (builtPkgLocation: string, opts: {pkgId: string, engine: string}): Promise<void>,
}

export type FetchPackageToStoreFunction = (
  opts: FetchPackageToStoreOptions,
) => {
  fetchingFiles: Promise<PackageFilesResponse>,
  fetchingRawManifest?: Promise<PackageJson>,
  finishing: Promise<void>,
  inStoreLocation: string,
}

export interface FetchPackageToStoreOptions {
  fetchRawManifest?: boolean,
  force: boolean,
  pkgName?: string,
  pkgId: string,
  prefix: string,
  resolution: Resolution,
  verifyStoreIntegrity: boolean, // TODO: this should be a context field
}

export type ImportPackageFunction = (
  from: string,
  to: string,
  opts: {
    filesResponse: PackageFilesResponse,
    force: boolean,
  },
) => Promise<void>

export interface PackageFilesResponse {
  fromStore: boolean,
  filenames: string[],
}

export type RequestPackageFunction = (
  wantedDependency: WantedDependency,
  options: RequestPackageOptions,
) => Promise<PackageResponse>

export interface RequestPackageOptions {
  defaultTag?: string,
  skipFetch?: boolean,
  downloadPriority: number,
  loggedPkg: LoggedPkg,
  currentPkgId?: string,
  prefix: string,
  registry: string,
  shrinkwrapResolution?: Resolution,
  update?: boolean,
  verifyStoreIntegrity: boolean, // TODO: this should be a context field
  preferredVersions: {
    [packageName: string]: {
      selector: string,
      type: 'version' | 'range' | 'tag',
    },
  },
  localPackages?: LocalPackages,
  sideEffectsCache?: boolean,
}

export interface LoggedPkg {
  rawSpec: string,
  name?: string,
  dependentId?: string,
}

export type PackageResponse = {
  body: {
    isLocal: true,
    resolution: DirectoryResolution,
    manifest: PackageManifest
    id: string,
    normalizedPref?: string,
    updated: boolean,
    resolvedVia?: string,
  },
} | (
  {
    fetchingFiles?: Promise<PackageFilesResponse>,
    finishing?: Promise<void>, // a package request is finished once its integrity is generated and saved
    body: {
      isLocal: false,
      inStoreLocation: string,
      cacheByEngine: Map<string, string>,
      id: string,
      resolution: Resolution,
      // This is useful for recommending updates.
      // If latest does not equal the version of the
      // resolved package, it is out-of-date.
      latest?: string,
      normalizedPref?: string,
      updated: boolean,
      resolvedVia?: string,
    },
  } & (
    {
      fetchingRawManifest: Promise<PackageJson>,
    } | {
      body: {
        manifest: PackageManifest,
        updated: boolean,
      },
    }
  )
)
