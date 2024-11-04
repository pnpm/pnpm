import fs from 'fs'
import util from 'util'
import { logger } from '@pnpm/logger'
import { getCacheFilePath } from './cacheFile'
import { type PackagesList } from './types'

export async function loadPackagesList (workspaceDir: string): Promise<PackagesList | undefined> {
  logger.debug({ msg: 'loading packages list' })
  const cacheFile = getCacheFilePath(workspaceDir)
  let cacheFileContent: string
  try {
    cacheFileContent = await fs.promises.readFile(cacheFile, 'utf-8')
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
      return undefined
    }
    throw error
  }
  return JSON.parse(cacheFileContent) as PackagesList
}
