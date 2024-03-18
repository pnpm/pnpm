import '@total-typescript/ts-reset'
import type {
  DirectoryResolution,
  PreferredVersions,
  Resolution,
  WantedDependency,
  WorkspacePackages,
} from '@pnpm/resolver-base'
import type {
  ImportPackageFunction,
  ImportPackageFunctionAsync,
  PackageFileInfo,
  PackageFilesResponse,
  ResolvedFrom,
} from '@pnpm/cafs-types'
import type {
  SupportedArchitectures,
  DependencyManifest,
  PackageManifest,
} from '@pnpm/types'

export type {
  PackageFileInfo,
  PackageFilesResponse,
  ImportPackageFunction,
  ImportPackageFunctionAsync,
}

export * from '@pnpm/resolver-base'
export type BundledManifest = Pick<
  DependencyManifest,
  | 'bin'
  | 'bundledDependencies'
  | 'bundleDependencies'
  | 'dependencies'
  | 'directories'
  | 'engines'
  | 'name'
  | 'optionalDependencies'
  | 'os'
  | 'peerDependencies'
  | 'peerDependenciesMeta'
  | 'scripts'
  | 'version'
>

export interface UploadPkgToStoreOpts {
  filesIndexFile: string
  sideEffectsCacheKey: string
}

export type UploadPkgToStore = (
  builtPkgLocation: string,
  opts: UploadPkgToStoreOpts
) => Promise<void>

export interface StoreController {
  requestPackage: RequestPackageFunction
  fetchPackage: FetchPackageToStoreFunction | FetchPackageToStoreFunctionAsync
  getFilesIndexFilePath: GetFilesIndexFilePath
  importPackage: ImportPackageFunctionAsync
  close: () => Promise<void>
  prune: (removeAlienFiles?: boolean) => Promise<void>
  upload: UploadPkgToStore
}

export interface PkgRequestFetchResult {
  bundledManifest?: BundledManifest | undefined
  files: PackageFilesResponse
}

export type FetchPackageToStoreFunction = (
  opts: FetchPackageToStoreOptions
) => {
  filesIndexFile: string
  fetching: () => Promise<PkgRequestFetchResult>
}

export type FetchPackageToStoreFunctionAsync = (
  opts: FetchPackageToStoreOptions
) => Promise<{
  filesIndexFile: string
  fetching: () => Promise<PkgRequestFetchResult>
}>

export type GetFilesIndexFilePath = (
  opts: Pick<FetchPackageToStoreOptions, 'pkg' | 'ignoreScripts'>
) => {
  filesIndexFile: string
  target: string
}

export interface PkgNameVersion {
  name?: string | undefined
  version?: string | undefined
}

export interface FetchPackageToStoreOptions {
  fetchRawManifest?: boolean | undefined
  force: boolean
  ignoreScripts?: boolean | undefined
  lockfileDir: string
  pkg: PkgNameVersion & {
    id: string
    resolution: Resolution
  }
  /**
   * Expected package is the package name and version that are found in the lockfile.
   */
  expectedPkg?: PkgNameVersion | undefined
  onFetchError?: OnFetchError | undefined
}

export type OnFetchError = (error: Error) => Error

export type RequestPackageFunction = (
  wantedDependency: WantedDependency & { optional?: boolean | undefined },
  options: RequestPackageOptions
) => Promise<PackageResponse>

export interface RequestPackageOptions {
  alwaysTryWorkspacePackages?: boolean
  currentPkg?: {
    id?: string | undefined
    resolution?: Resolution | undefined
  } | undefined
  /**
   * Expected package is the package name and version that are found in the lockfile.
   */
  expectedPkg?: PkgNameVersion | undefined
  defaultTag?: string | undefined
  pickLowestVersion?: boolean | undefined
  publishedBy?: Date | undefined
  downloadPriority: number
  ignoreScripts?: boolean | undefined
  projectDir: string
  lockfileDir: string
  preferredVersions: PreferredVersions
  preferWorkspacePackages?: boolean | undefined
  registry: string
  sideEffectsCache?: boolean | undefined
  skipFetch?: boolean | undefined
  update?: boolean | undefined
  workspacePackages?: WorkspacePackages | undefined
  forceResolve?: boolean | undefined
  supportedArchitectures?: SupportedArchitectures | undefined
  onFetchError?: OnFetchError | undefined
  updateToLatest?: boolean | undefined
}

export type BundledManifestFunction = () => Promise<BundledManifest | undefined>

export interface PackageResponse {
  fetching?: (() => Promise<PkgRequestFetchResult>) | undefined
  filesIndexFile?: string | undefined
  body: {
    isLocal: boolean
    isInstallable?: boolean | undefined
    resolution: Resolution
    manifest?: PackageManifest | undefined
    id: string
    normalizedPref?: string | undefined
    updated: boolean
    publishedAt?: string | undefined
    resolvedVia?: string | undefined
    // This is useful for recommending updates.
    // If latest does not equal the version of the
    // resolved package, it is out-of-date.
    latest?: string | undefined
  } & (
    | {
      isLocal: true
      resolution: DirectoryResolution
    }
    | {
      isLocal: false
    }
  )
}

export type FilesMap = Record<string, string>

export interface ImportOptions {
  disableRelinkLocalDirDeps?: boolean | undefined
  filesMap: FilesMap
  force: boolean
  resolvedFrom: ResolvedFrom
  keepModulesDir?: boolean | undefined
}

export type ImportIndexedPackage = (
  to: string,
  opts: ImportOptions
) => string | undefined

export type ImportIndexedPackageAsync = (
  to: string,
  opts: ImportOptions
) => Promise<string | undefined>
