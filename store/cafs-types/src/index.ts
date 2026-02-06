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
}

export type ResolvedFrom = 'store' | 'local-dir' | 'remote'

export type FilesMap = Map<string, string>

export interface PackageFilesResponse {
  resolvedFrom: ResolvedFrom
  filesMap: FilesMap
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  // Pre-calculated file location maps for side effects, avoiding recalculation during import
  sideEffectsMaps?: Map<string, { added?: FilesMap, deleted?: string[] }>
  requiresBuild: boolean
}

export interface ImportPackageOpts {
  disableRelinkLocalDirDeps?: boolean
  requiresBuild?: boolean
  sideEffectsCacheKey?: string
  filesResponse: PackageFilesResponse
  force: boolean
  keepModulesDir?: boolean
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
  addFilesFromTarball: (buffer: Buffer) => AddToStoreResult
  addFile: (buffer: Buffer, mode: number) => FileWriteResult
  getIndexFilePathInCafs: (integrity: string, pkgId: string) => string
  getFilePathByModeInCafs: (digest: string, mode: number) => string
  importPackage: ImportPackageFunction
  tempDir: () => Promise<string>
}
