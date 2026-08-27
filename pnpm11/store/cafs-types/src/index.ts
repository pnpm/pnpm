import type { DependencyManifest } from '@pnpm/types'

export type PackageFiles = Map<string, PackageFileInfo>

export interface PackageFileInfo {
  checkedAt?: number // Nullable for backward compatibility
  digest: string
  mode: number
  size: number
}

export type SideEffects = Map<string, SideEffectsDiff>

export interface SideEffectsDiff {
  deleted?: string[]
  added?: PackageFiles
  remoteOrigin?: RemoteSideEffectsOrigin
}

export type RemoteSideEffectsOwner =
  | { type: 'organization', name: string }
  | { type: 'publisher', package: string }

export interface RemoteSideEffectsBuilderProfile {
  imageDigest?: string
  architectureBaseline: string
  environment: Record<string, string>
}

export interface RemoteSideEffectsEnvelope {
  algorithm: string
  keyId: string
  payload: string
  signature: string
}

export interface RemoteSideEffectsOrigin {
  channel: string
  owner: RemoteSideEffectsOwner
  signerKeyId: string
  builderProfile: RemoteSideEffectsBuilderProfile
  envelope: RemoteSideEffectsEnvelope
  verification: 'verified'
}

export type RemoteSideEffectsQuarantine = Map<string, string[]>

export type ResolvedFrom = 'store' | 'local-dir' | 'remote'

export type FilesMap = Map<string, string>

export interface PackageFilesResponse {
  resolvedFrom: ResolvedFrom
  filesMap: FilesMap
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  // Pre-calculated file location maps for side effects, avoiding recalculation during import
  sideEffectsMaps?: Map<string, { added?: FilesMap, deleted?: string[] }>
  sideEffectsDiffs?: SideEffects
  remoteSideEffectsQuarantine?: RemoteSideEffectsQuarantine
  requiresBuild: boolean
  /** Whether preparing a git package required lifecycle scripts before these files were stored. */
  requiresPrepare?: boolean
}

export interface ImportPackageOpts {
  disableRelinkLocalDirDeps?: boolean
  requiresBuild?: boolean
  sideEffectsCacheKey?: string
  filesResponse: PackageFilesResponse
  force: boolean
  keepModulesDir?: boolean
  safeToSkip?: boolean
}

export type ImportPackageFunction = (
  to: string,
  opts: ImportPackageOpts
) => { isBuilt: boolean, importMethod: undefined | string }

export type ImportPackageFunctionAsync = (
  to: string,
  opts: ImportPackageOpts
) => Promise<{ isBuilt: boolean, importMethod: undefined | string }>

export type FileType = 'exec' | 'nonexec'

export type FilesIndex = Map<string, {
  mode: number
  size: number
} & FileWriteResult>

export interface FileWriteResult {
  checkedAt: number
  filePath: string
  digest: string
}

export interface AddToStoreResult {
  filesIndex: FilesIndex
  manifest?: DependencyManifest
}

export interface Cafs {
  storeDir: string
  addFilesFromDir: (dir: string) => AddToStoreResult
  addFilesFromTarball: (buffer: Buffer, readManifest?: boolean, ignore?: (filename: string) => boolean) => AddToStoreResult
  addFile: (buffer: Buffer, mode: number) => FileWriteResult
  getFilePathByModeInCafs: (digest: string, mode: number) => string
  importPackage: ImportPackageFunction
  tempDir: () => Promise<string>
}
