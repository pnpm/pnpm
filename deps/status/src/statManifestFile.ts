import fs from 'fs'
import path from 'path'
import util from 'util'
import { MANIFEST_BASE_NAMES } from '@pnpm/constants'

export async function statManifestFile (projectRootDir: string): Promise<fs.Stats | undefined> {
  const attempts = await Promise.all(MANIFEST_BASE_NAMES.map((baseName) => {
    const manifestPath = path.join(projectRootDir, baseName)
    return safeStat(manifestPath)
  }))
  return attempts.find(stats => stats != null)
}

export async function safeStat (filePath: string): Promise<fs.Stats | undefined> {
  try {
    return await fs.promises.stat(filePath)
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
      return undefined
    }
    throw error
  }
}

export function safeStatSync (filePath: string): fs.Stats | undefined {
  try {
    return fs.statSync(filePath)
  } catch (error) {
    if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
      return undefined
    }
    throw error
  }
}
