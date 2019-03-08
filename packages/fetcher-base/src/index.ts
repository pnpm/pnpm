import { Resolution } from '@pnpm/resolver-base'

export interface FetchOptions {
  cachedTarballLocation: string,
  prefix: string,
  onStart?: (totalSize: number | null, attempt: number) => void,
  onProgress?: (downloaded: number) => void,
}

export type FetchFunction = (
  resolution: Resolution,
  targetFolder: string,
  opts: FetchOptions,
) => Promise<FetchResult>

export interface FetchResult {
  filesIndex: FilesIndex,
  tempLocation: string,
}

export interface FilesIndex {
  [filename: string]: {
    size: number,
    generatingIntegrity: Promise<string>,
  },
}
