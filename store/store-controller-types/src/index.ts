import {
  type PkgResolutionId,
  type DirectoryResolution,
  type PreferredVersions,
  type Resolution,
  type WantedDependency,
  type WorkspacePackages,
} from '@pnpm/resolver-base'
import {
  type FilesMap,
  type ImportPackageFunction,
  type ImportPackageFunctionAsync,
  type PackageFileInfo,
  type PackageFilesResponse,
  type ResolvedFrom,
} from '@pnpm/cafs-types'
import {
  type AllowBuild,
  type BundledManifest,
  type SupportedArchitectures,
  type PackageManifest,
  type PinnedVersion,
  type PackageVersionPolicy,
  type TrustPolicy,
} from '@pnpm/types'

export type { PackageFileInfo, PackageFilesResponse, ImportPackageFunction, ImportPackageFunctionAsync, FilesMap }

export * from '@pnpm/resolver-base'
export type { BundledManifest }

export interface UploadPkgToStoreOpts {
  filesIndexFile: string
  sideEffectsCacheKey: string
}

export type UploadPkgToStore = (builtPkgLocation: string, opts: UploadPkgToStoreOpts) => Promise<void>

export interface StoreController {
  requestPackage: RequestPackageFunction
  fetchPackage: FetchPackageToStoreFunction | FetchPackageToStoreFunctionAsync
  getFilesIndexFilePath: GetFilesIndexFilePath
  importPackage: ImportPackageFunctionAsync
  close: () => Promise<void>
  prune: (removeAlienFiles?: boolean) => Promise<void>
  upload: UploadPkgToStore
  clearResolutionCache: () => void
}

export interface PkgRequestFetchResult {
  bundledManifest?: BundledManifest
  files: PackageFilesResponse
  integrity?: string
}

export interface FetchResponse {
  filesIndexFile: string
  fetching: () => Promise<PkgRequestFetchResult>
}

export type FetchPackageToStoreFunction = (opts: FetchPackageToStoreOptions) => FetchResponse

export type FetchPackageToStoreFunctionAsync = (opts: FetchPackageToStoreOptions) => Promise<FetchResponse>

export type GetFilesIndexFilePath = (opts: Pick<FetchPackageToStoreOptions, 'pkg' | 'ignoreScripts'>) => {
  filesIndexFile: string
  target: string
}

export interface PkgNameVersion {
  name?: string
  version?: string
}

export interface FetchPackageToStoreOptions {
  allowBuild?: AllowBuild
  fetchRawManifest?: boolean
  force: boolean
  ignoreScripts?: boolean
  lockfileDir: string
  pkg: PkgNameVersion & {
    id: string
    resolution: Resolution
  }
  onFetchError?: OnFetchError
  supportedArchitectures?: SupportedArchitectures
}

export type OnFetchError = (error: Error) => Error

export type RequestPackageFunction = (
  wantedDependency: WantedDependency & { optional?: boolean },
  options: RequestPackageOptions
) => Promise<PackageResponse>

export interface RequestPackageOptions {
  allowBuild?: AllowBuild
  alwaysTryWorkspacePackages?: boolean
  currentPkg?: {
    id?: PkgResolutionId
    name?: string
    resolution?: Resolution
    version?: string
  }
  /**
   * Expected package is the package name and version that are found in the lockfile.
   */
  expectedPkg?: PkgNameVersion
  defaultTag?: string
  pickLowestVersion?: boolean
  publishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
  downloadPriority: number
  ignoreScripts?: boolean
  projectDir: string
  lockfileDir: string
  preferredVersions: PreferredVersions
  preferWorkspacePackages?: boolean
  sideEffectsCache?: boolean
  skipFetch?: boolean
  update?: false | 'compatible' | 'latest'
  workspacePackages?: WorkspacePackages
  forceResolve?: boolean
  supportedArchitectures?: SupportedArchitectures
  onFetchError?: OnFetchError
  injectWorkspacePackages?: boolean
  calcSpecifier?: boolean
  pinnedVersion?: PinnedVersion
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: PackageVersionPolicy
  trustPolicyIgnoreAfter?: number
}

export type BundledManifestFunction = () => Promise<BundledManifest | undefined>

export interface PackageResponse {
  fetching?: () => Promise<PkgRequestFetchResult>
  filesIndexFile?: string
  body: {
    isLocal: boolean
    isInstallable?: boolean
    resolution: Resolution
    manifest?: PackageManifest
    id: PkgResolutionId
    normalizedBareSpecifier?: string
    updated: boolean
    publishedAt?: string
    resolvedVia?: string
    // This is useful for recommending updates.
    // If latest does not equal the version of the
    // resolved package, it is out-of-date.
    latest?: string
    alias?: string
  } & (
    {
      isLocal: true
      resolution: DirectoryResolution
    } | {
      isLocal: false
    }
  )
}

export interface ImportOptions {
  disableRelinkLocalDirDeps?: boolean
  filesMap: FilesMap
  force: boolean
  resolvedFrom: ResolvedFrom
  keepModulesDir?: boolean
}

export type ImportIndexedPackage = (to: string, opts: ImportOptions) => string | undefined

export type ImportIndexedPackageAsync = (to: string, opts: ImportOptions) => Promise<string | undefined>
