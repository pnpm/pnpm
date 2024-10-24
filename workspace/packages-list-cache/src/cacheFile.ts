import path from 'path'
import { createShortHash as cacheID } from '@pnpm/crypto.hash'

export interface CacheFilePathOptions {
  cacheDir: string
  workspaceDir: string
}

export const getCacheFilePath = (opts: CacheFilePathOptions): string =>
  path.join(opts.cacheDir, 'workspace-packages-lists', 'v1', `${cacheID(opts.workspaceDir)}.json`)
