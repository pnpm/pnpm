import type { LockfileObject } from '@pnpm/lockfile.types'

export interface MissingFile {
  digest: string
  size: number
  executable: boolean
  /** Absolute path to the file in the server's CAFS */
  cafsPath: string
}

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
