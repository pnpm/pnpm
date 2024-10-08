import fs from 'fs'
import util from 'util'
import { getCacheFilePath } from './cacheFile'
import { isPackagesList } from './isPackagesList'
import { type PackagesList } from './types'

export interface LoadPackagesListOptions {
  cacheDir: string
  workspaceDir: string
}

export async function loadPackagesList (opts: LoadPackagesListOptions): Promise<PackagesList | undefined> {
  const cacheFile = getCacheFilePath(opts)
  let cacheFileContent: string
  try {
    cacheFileContent = await fs.promises.readFile(cacheFile, 'utf-8')
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
      return undefined
    }
    throw error
  }
  const value: unknown = JSON.parse(cacheFileContent)
  if (!isPackagesList(value)) return undefined
  if (value.workspaceDir !== opts.workspaceDir) return undefined // sometimes, collision happens
  return value
}
