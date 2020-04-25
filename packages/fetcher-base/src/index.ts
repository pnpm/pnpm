import { Resolution } from '@pnpm/resolver-base'
import { Integrity } from 'ssri'

export type Cafs = {
  addFilesFromDir: (dir: string) => Promise<FilesIndex>,
  addFilesFromTarball: (stream: NodeJS.ReadableStream) => Promise<FilesIndex>,
}

export interface FetchOptions {
  cachedTarballLocation: string,
  lockfileDir: string,
  onStart?: (totalSize: number | null, attempt: number) => void,
  onProgress?: (downloaded: number) => void,
}

export type FetchFunction = (
  cafs: Cafs,
  resolution: Resolution,
  opts: FetchOptions,
) => Promise<FetchResult>

export interface FetchResult {
  filesIndex: FilesIndex,
}

export interface FilesIndex {
  [filename: string]: {
    size: number,
    generatingIntegrity: Promise<Integrity>,
  },
}
