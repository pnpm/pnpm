import type { IntegrityLike } from 'ssri'
import type { DependencyManifest } from '@pnpm/types'

export type PackageFiles = Record<string, PackageFileInfo>

export interface PackageFileInfo {
  checkedAt?: number // Nullable for backward compatibility
  integrity: string
  mode: number
  size: number
}

export type SideEffects = Record<string, SideEffectsDiff>

export interface SideEffectsDiff {
  deleted?: string[]
  added?: PackageFiles
}

export type ResolvedFrom = 'store' | 'local-dir' | 'remote'

export type PackageFilesResponse = {
  resolvedFrom: ResolvedFrom
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  sideEffects?: SideEffects
  requiresBuild: boolean
} & ({
  unprocessed?: false
  filesIndex: Record<string, string>
} | {
  unprocessed: true
  filesIndex: PackageFiles
})

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

export type FileType = 'exec' | 'nonexec' | 'index'

export interface FilesIndex {
  [filename: string]: {
    mode: number
    size: number
  } & FileWriteResult
}

export interface FileWriteResult {
  checkedAt: number
  filePath: string
  integrity: IntegrityLike
}

export interface AddToStoreResult {
  filesIndex: FilesIndex
  manifest?: DependencyManifest
}

export interface Cafs {
  cafsDir: string
  addFilesFromDir: (dir: string) => AddToStoreResult
  addFilesFromTarball: (buffer: Buffer) => AddToStoreResult
  getIndexFilePathInCafs: (integrity: string | IntegrityLike, fileType: FileType) => string
  getFilePathByModeInCafs: (integrity: string | IntegrityLike, mode: number) => string
  importPackage: ImportPackageFunction
  tempDir: () => Promise<string>
}
