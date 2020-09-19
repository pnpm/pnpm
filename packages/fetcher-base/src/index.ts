import { Resolution } from '@pnpm/resolver-base'
import { DependencyManifest } from '@pnpm/types'
import { IntegrityLike } from 'ssri'

export interface Cafs {
  addFilesFromDir: (dir: string, manifest?: DeferredManifestPromise) => Promise<FilesIndex>
  addFilesFromTarball: (stream: NodeJS.ReadableStream, manifest?: DeferredManifestPromise) => Promise<FilesIndex>
}

export interface FetchOptions {
  manifest?: DeferredManifestPromise
  lockfileDir: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
}

export interface DeferredManifestPromise {
  resolve: (manifest: DependencyManifest) => void
  reject: (err: Error) => void
}

export type FetchFunction = (
  cafs: Cafs,
  resolution: Resolution,
  opts: FetchOptions
) => Promise<FetchResult>

export interface FetchResult {
  filesIndex: FilesIndex
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
