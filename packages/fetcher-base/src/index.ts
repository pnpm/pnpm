import { Resolution } from '@pnpm/resolver-base'
import { DependencyManifest } from '@pnpm/types'
import { IntegrityLike } from 'ssri'

export interface PackageFileInfo {
  checkedAt?: number // Nullable for backward compatibility
  integrity: string
  mode: number
  size: number
}

export type PackageFilesResponse = {
  fromStore: boolean
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone'
  sideEffects?: Record<string, Record<string, PackageFileInfo>>
} & ({
  local: true
  filesIndex: Record<string, string>
} | {
  local?: false
  filesIndex: Record<string, PackageFileInfo>
})

export type ImportPackageFunction = (
  to: string,
  opts: {
    targetEngine?: string
    filesResponse: PackageFilesResponse
    force: boolean
  }
) => Promise<{ isBuilt: boolean, importMethod: undefined | string }>

export interface Cafs {
  addFilesFromDir: (dir: string, manifest?: DeferredManifestPromise) => Promise<FilesIndex>
  addFilesFromTarball: (stream: NodeJS.ReadableStream, manifest?: DeferredManifestPromise) => Promise<FilesIndex>
  importPackage: ImportPackageFunction
  tempDir: () => Promise<string>
}

export interface FetchOptions {
  manifest?: DeferredManifestPromise
  lockfileDir: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
}

export interface DeferredManifestPromise {
  resolve: (manifest: DependencyManifest | undefined) => void
  reject: (err: Error) => void
}

export type FetchFunction = (
  cafs: Cafs,
  resolution: Resolution,
  opts: FetchOptions
) => Promise<FetchResult>

export type FetchResult = {
  local?: false
  filesIndex: FilesIndex
} | {
  local: true
  filesIndex: Record<string, string>
}

export interface FileWriteResult {
  checkedAt: number
  integrity: IntegrityLike
}

export interface FilesIndex {
  [filename: string]: {
    mode: number
    size: number
    writeResult: Promise<FileWriteResult>
  }
}
