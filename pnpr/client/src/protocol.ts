import type { LockfileObject } from '@pnpm/lockfile.types'

export interface ResponseMetadata {
  lockfile: LockfileObject
  stats: {
    totalPackages: number
  }
}
