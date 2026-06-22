import type {
  BinaryFetcher,
  DirectoryFetcher,
  FetchFunction,
  GitFetcher,
} from '@pnpm/fetching.fetcher-base'
import type {
  DirectoryResolution,
  PkgResolutionId,
  PreferredVersions,
  Resolution,
  ResolutionPolicyViolation,
  WantedDependency,
  WorkspacePackages,
} from '@pnpm/resolving.resolver-base'
import type {
  FilesMap,
  ImportPackageFunction,
  ImportPackageFunctionAsync,
  PackageFileInfo,
  PackageFilesResponse,
  ResolvedFrom,
} from '@pnpm/store.cafs-types'
import type {
  AllowBuild,
  BundledManifest,
  PackageManifest,
  PackageVersionPolicy,
  PinnedVersion,
  SupportedArchitectures,
  TrustPolicy,
} from '@pnpm/types'

export type { FilesMap, ImportPackageFunction, ImportPackageFunctionAsync, PackageFileInfo, PackageFilesResponse }

export * from '@pnpm/resolving.resolver-base'
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

type SelectedFetcher = FetchFunction | DirectoryFetcher | GitFetcher | BinaryFetcher

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
  /**
   * The resolution can't be completed without a fresh download (e.g. a registry tarball
   * whose integrity must be computed from the bytes), so the store copy must not be
   * reused. Determined by the fetcher's `resolutionNeedsFetch`.
   */
  populateMissingIntegrity?: boolean
  /**
   * In-process callers may pass the fetcher they already selected for this resolution.
   * Omitted when the fetcher has to be selected at fetch time, such as `variations`.
   */
  pickedFetcher?: SelectedFetcher
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
    publishedAt?: string
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
  updateChecksums?: boolean
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
  /**
   * The resolution can't be completed without awaiting `fetching` — e.g. a registry
   * tarball whose integrity is computed from the downloaded bytes. Set by the fetcher's
   * `resolutionNeedsFetch`. Callers that read the resolution before fetching (the lockfile
   * snapshot, virtual-store paths) must await `fetching` first for these.
   */
  resolutionNeedsFetch?: boolean
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
    /**
     * Forwarded from the resolver's `ResolveResult.policyViolation`.
     * The caller (deps-resolver) aggregates these per-pick into a
     * single set the install command can react to — see
     * `ResolutionPolicyViolation` in `@pnpm/resolving.resolver-base`.
     */
    policyViolation?: ResolutionPolicyViolation
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
  safeToSkip?: boolean
}

export type ImportIndexedPackage = (to: string, opts: ImportOptions) => string | undefined

export type ImportIndexedPackageAsync = (to: string, opts: ImportOptions) => Promise<string | undefined>
