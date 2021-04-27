import {
  DirectoryResolution,
  PreferredVersions,
  Resolution,
  WantedDependency,
  WorkspacePackages,
} from '@pnpm/resolver-base'
import {
  ImportPackageFunction,
  PackageFileInfo,
  PackageFilesResponse,
} from '@pnpm/fetcher-base'
import {
  DependencyManifest,
  PackageManifest,
} from '@pnpm/types'

export { PackageFileInfo, PackageFilesResponse, ImportPackageFunction }

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

export interface StoreController {
  requestPackage: RequestPackageFunction
  fetchPackage: FetchPackageToStoreFunction
  importPackage: ImportPackageFunction
  close: () => Promise<void>
  prune: () => Promise<void>
  upload: (builtPkgLocation: string, opts: {filesIndexFile: string, engine: string}) => Promise<void>
}

export type FetchPackageToStoreFunction = (
  opts: FetchPackageToStoreOptions
) => {
  bundledManifest?: () => Promise<BundledManifest>
  filesIndexFile: string
  files: () => Promise<PackageFilesResponse>
  finishing: () => Promise<void>
}

export interface FetchPackageToStoreOptions {
  fetchRawManifest?: boolean
  force: boolean
  lockfileDir: string
  pkg: {
    id: string
    name?: string
    version?: string
    resolution: Resolution
  }
}

export type RequestPackageFunction = (
  wantedDependency: WantedDependency,
  options: RequestPackageOptions
) => Promise<PackageResponse>

export interface RequestPackageOptions {
  alwaysTryWorkspacePackages?: boolean
  currentPkg?: {
    id?: string
    name?: string
    version?: string
    resolution?: Resolution
  }
  defaultTag?: string
  downloadPriority: number
  projectDir: string
  lockfileDir: string
  preferredVersions: PreferredVersions
  preferWorkspacePackages?: boolean
  registry: string
  sideEffectsCache?: boolean
  skipFetch?: boolean
  update?: boolean
  workspacePackages?: WorkspacePackages
}

export interface PackageResponse {
  bundledManifest?: () => Promise<BundledManifest>
  files?: () => Promise<PackageFilesResponse>
  filesIndexFile?: string
  finishing?: () => Promise<void> // a package request is finished once its integrity is generated and saved
  body: {
    isLocal: boolean
    resolution: Resolution
    manifest?: PackageManifest
    id: string
    normalizedPref?: string
    updated: boolean
    resolvedVia?: string
    // This is useful for recommending updates.
    // If latest does not equal the version of the
    // resolved package, it is out-of-date.
    latest?: string
  } & (
    {
      isLocal: true
      resolution: DirectoryResolution
    } | {
      isLocal: false
    }
  )
}
