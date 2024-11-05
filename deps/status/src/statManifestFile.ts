import fs from 'fs'
import path from 'path'
import util from 'util'
import { MANIFEST_BASE_NAMES } from '@pnpm/constants'

export async function statManifestFile (projectRootDir: string): Promise<fs.Stats | undefined> {
  const attempts = await Promise.all(MANIFEST_BASE_NAMES.map(async baseName => {
    const manifestPath = path.join(projectRootDir, baseName)
    let stats: fs.Stats
    try {
      stats = await fs.promises.stat(manifestPath)
    } catch (error) {
      if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') {
        return undefined
      }
      throw error
    }
    return stats
  }))
  return attempts.find(stats => stats != null)
}
