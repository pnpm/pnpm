import type { IntegrityLike } from 'ssri'
import type { DependencyManifest } from '@pnpm/types'

export interface DeferredManifestPromise {
  resolve: (manifest: DependencyManifest | undefined) => void
  reject: (err: Error) => void
}

export interface PackageFileInfo {
  checkedAt?: number // Nullable for backward compatibility
  integrity: string
  mode: number
  size: number
}

export type PackageFilesResponse = {
  fromStore: boolean
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  sideEffects?: Record<string, Record<string, PackageFileInfo>>
} & ({
  local: true
  filesIndex: Record<string, string>
} | {
  local?: false
  filesIndex: Record<string, PackageFileInfo>
})

export interface ImportPackageOpts {
  requiresBuild?: boolean
  sideEffectsCacheKey?: string
  filesResponse: PackageFilesResponse
  force: boolean
}

export type ImportPackageFunction = (
  to: string,
  opts: ImportPackageOpts
) => Promise<{ isBuilt: boolean, importMethod: undefined | string }>

export type FileType = 'exec' | 'nonexec' | 'index'

export interface FilesIndex {
  [filename: string]: {
    mode: number
    size: number
    writeResult: Promise<FileWriteResult>
  }
}

export interface FileWriteResult {
  checkedAt: number
  integrity: IntegrityLike
}

export interface Cafs {
  cafsDir: string
  addFilesFromDir: (dir: string, manifest?: DeferredManifestPromise) => Promise<FilesIndex>
  addFilesFromTarball: (stream: NodeJS.ReadableStream, manifest?: DeferredManifestPromise) => Promise<FilesIndex>
  getFilePathInCafs: (integrity: string | IntegrityLike, fileType: FileType) => string
  getFilePathByModeInCafs: (integrity: string | IntegrityLike, mode: number) => string
  importPackage: ImportPackageFunction
  tempDir: () => Promise<string>
}
