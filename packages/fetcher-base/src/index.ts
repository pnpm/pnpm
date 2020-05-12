import { Resolution } from '@pnpm/resolver-base'
import { DependencyManifest } from '@pnpm/types'
import { Integrity } from 'ssri'

export type Cafs = {
  addFilesFromDir: (dir: string, manifest?: DeferredManifestPromise) => Promise<FilesIndex>,
  addFilesFromTarball: (stream: NodeJS.ReadableStream, manifest?: DeferredManifestPromise) => Promise<FilesIndex>,
}

export interface FetchOptions {
  cachedTarballLocation: string,
  manifest?: DeferredManifestPromise,
  lockfileDir: string,
  onStart?: (totalSize: number | null, attempt: number) => void,
  onProgress?: (downloaded: number) => void,
}

export type DeferredManifestPromise = {
  resolve: (manifest: DependencyManifest) => void,
  reject: (err: Error) => void,
}

export type FetchFunction = (
  cafs: Cafs,
  resolution: Resolution,
  opts: FetchOptions
) => Promise<FetchResult>

export interface FetchResult {
  filesIndex: FilesIndex,
}

export interface FilesIndex {
  [filename: string]: {
    mode: number,
    size: number,
    generatingIntegrity: Promise<Integrity>,
  },
}
