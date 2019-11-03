import { Resolution } from '@pnpm/resolver-base'
import { IntegrityMap } from 'ssri'

export interface FetchOptions {
  cachedTarballLocation: string,
  lockfileDir: string,
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
    generatingIntegrity?: Promise<IntegrityMap>,
  },
}
