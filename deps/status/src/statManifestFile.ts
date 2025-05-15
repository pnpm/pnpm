import type fs from 'fs'
import path from 'path'
import { MANIFEST_BASE_NAMES } from '@pnpm/constants'
import { safeStat } from './safeStat'

export async function statManifestFile (projectRootDir: string): Promise<fs.Stats | undefined> {
  const attempts = await Promise.all(MANIFEST_BASE_NAMES.map((baseName) => {
    const manifestPath = path.join(projectRootDir, baseName)
    return safeStat(manifestPath)
  }))
  return attempts.find(stats => stats != null)
}
