import fs from 'fs'
import util from 'util'
import { logger } from '@pnpm/logger'
import { getCacheFilePath } from './cacheFile'
import { type PackagesList } from './types'

export function loadPackagesList (workspaceDir: string): PackagesList | undefined {
  logger.debug({ msg: 'loading packages list' })
  const cacheFile = getCacheFilePath(workspaceDir)
  let cacheFileContent: string
  try {
    cacheFileContent = fs.readFileSync(cacheFile, 'utf-8')
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
      return undefined
    }
    throw error
  }
  return JSON.parse(cacheFileContent) as PackagesList
}
