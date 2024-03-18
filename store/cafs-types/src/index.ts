import '@total-typescript/ts-reset'
import type { IntegrityLike } from 'ssri'
import type { DependencyManifest } from '@pnpm/types'

export interface PackageFileInfo {
  checkedAt?: number | undefined // Nullable for backward compatibility
  integrity: string
  mode: number
  size: number
}

export type ResolvedFrom = 'store' | 'local-dir' | 'remote'

export type PackageFilesResponse = {
  resolvedFrom: ResolvedFrom
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  sideEffects?: Record<string, Record<string, PackageFileInfo>>
} & (
  | {
    unprocessed?: false | undefined
    filesIndex: Record<string, string>
  }
  | {
    unprocessed: true
    filesIndex: Record<string, PackageFileInfo>
  }
)

export interface ImportPackageOpts {
  disableRelinkLocalDirDeps?: boolean | undefined
  requiresBuild?: boolean | undefined
  sideEffectsCacheKey?: string | undefined
  filesResponse: PackageFilesResponse
  force: boolean
  keepModulesDir?: boolean | undefined
}

export type ImportPackageFunction = (
  to: string,
  opts: ImportPackageOpts
) => { isBuilt: boolean; importMethod?: undefined | string }

export type ImportPackageFunctionAsync = (
  to: string,
  opts: ImportPackageOpts
) => Promise<{ isBuilt: boolean; importMethod?: undefined | string }>

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
  manifest?: DependencyManifest | undefined
}

export interface Cafs {
  cafsDir: string
  addFilesFromDir: (dir: string) => AddToStoreResult
  addFilesFromTarball: (buffer: Buffer) => AddToStoreResult
  getFilePathInCafs: (
    integrity: string | IntegrityLike,
    fileType: FileType
  ) => string
  getFilePathByModeInCafs: (
    integrity: string | IntegrityLike,
    mode: number
  ) => string
  importPackage: ImportPackageFunction
  tempDir: () => Promise<string>
}
