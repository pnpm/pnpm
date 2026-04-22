import type { LockfileObject } from '@pnpm/lockfile.types'

export interface ResponseMetadata {
  lockfile: LockfileObject
  stats: {
    totalPackages: number
    alreadyInStore: number
    packagesToFetch: number
    filesInNewPackages: number
    filesAlreadyInCafs: number
    filesToDownload: number
    downloadBytes: number
  }
}
