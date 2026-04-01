import type { VersionSelectors } from '@pnpm/resolving.resolver-base'
import type { PackageFilesResponse } from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'

export interface PkgNameVersion {
  name?: string
  version?: string
}

export interface InitStoreMessage {
  type: 'init-store'
  storeDir: string
}

export interface TarballExtractMessage {
  type: 'extract'
  buffer: Buffer
  storeDir: string
  integrity?: string
  filesIndexFile: string
  readManifest?: boolean
  pkg?: PkgNameVersion
  appendManifest?: DependencyManifest
}

export interface LinkPkgMessage {
  type: 'link'
  storeDir: string
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  filesResponse: PackageFilesResponse
  sideEffectsCacheKey?: string | undefined
  targetDir: string
  requiresBuild?: boolean
  force: boolean
  keepModulesDir?: boolean
  disableRelinkLocalDirDeps?: boolean
  safeToSkip?: boolean
}

export interface SymlinkAllModulesMessage {
  type: 'symlinkAllModules'
  deps: Array<{
    children: Record<string, string>
    modules: string
    name: string
  }>
}

export interface AddDirToStoreMessage {
  type: 'add-dir'
  storeDir: string
  dir: string
  filesIndexFile: string
  sideEffectsCacheKey?: string
  readManifest?: boolean
  pkg?: PkgNameVersion
  appendManifest?: DependencyManifest
  files?: string[]
  includeNodeModules?: boolean
}

export interface ReadPkgFromCafsMessage {
  type: 'readPkgFromCafs'
  storeDir: string
  filesIndexFile: string
  readManifest: boolean
  verifyStoreIntegrity: boolean
  expectedPkg?: PkgNameVersion
  strictStorePkgContentCheck?: boolean
}

export interface HardLinkDirMessage {
  type: 'hardLinkDir'
  src: string
  destDirs: string[]
}

export interface ResolveMetadataSpec {
  type: 'tag' | 'version' | 'range'
  name: string
  fetchSpec: string
}

export interface ResolveMetadataMessage {
  type: 'resolve-metadata'
  cacheDir: string
  spec: ResolveMetadataSpec
  registry: string
  offline?: boolean
  preferOffline?: boolean
  pickLowestVersion?: boolean
  updateToLatest?: boolean
  fullMetadata?: boolean
  filterMetadata?: boolean
  strictPublishedByCheck?: boolean
  dryRun: boolean
  publishedBy?: number // timestamp — Date doesn't serialize
  publishedByExcludeResult?: boolean | string[] // pre-computed on main thread
  preferredVersionSelectors?: VersionSelectors
}

export interface ResolveMetadataHitResult {
  status: 'success'
  cacheHit: true
  pickedPackage: SerializedPackageInRegistry | null
  meta: SerializedPackageMeta
}

export interface ResolveMetadataMissResult {
  status: 'success'
  cacheHit: false
  needsFetch: true
  etag?: string
  modified?: string
}

export type ResolveMetadataResult = ResolveMetadataHitResult | ResolveMetadataMissResult

export interface ResolveMetadataDataMessage {
  type: 'resolve-metadata-data'
  cacheDir: string
  spec: ResolveMetadataSpec
  registry: string
  jsonText: string
  etag?: string
  notModified?: boolean
  fullMetadata?: boolean
  filterMetadata?: boolean
  strictPublishedByCheck?: boolean
  dryRun: boolean
  pickLowestVersion?: boolean
  updateToLatest?: boolean
  publishedBy?: number
  publishedByExcludeResult?: boolean | string[]
  preferredVersionSelectors?: VersionSelectors
}

export interface ResolveMetadataDataSuccessResult {
  status: 'success'
  pickedPackage: SerializedPackageInRegistry | null
  meta: SerializedPackageMeta
  needsFullRefetch?: false
}

export interface ResolveMetadataDataRefetchResult {
  status: 'success'
  needsFullRefetch: true
  meta: SerializedPackageMeta
}

export type ResolveMetadataDataResult = ResolveMetadataDataSuccessResult | ResolveMetadataDataRefetchResult

// Serializable versions of registry types (no Date objects, no functions)
export interface SerializedPackageMeta {
  name: string
  'dist-tags': Record<string, string>
  versions: Record<string, SerializedPackageInRegistry>
  time?: Record<string, string> & { unpublished?: { time: string, versions: string[] } }
  modified?: string
  cachedAt?: number
  etag?: string
}

export type SerializedPackageInRegistry = Record<string, unknown>
