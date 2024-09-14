import fs from 'fs'
import path from 'path'
import fastGlob from 'fast-glob'
import { getIndexFilePathInCafs } from '@pnpm/store.cafs'
import { type PackageMeta } from '@pnpm/npm-resolver'

interface CachedVersions {
  cachedVersions: string[]
  nonCachedVersions: string[]
  cachedAt?: string
  distTags: Record<string, string>
}

export async function cacheViewCmd (opts: { cacheDir: string, storeDir: string }, packageName: string): Promise<string> {
  const metaFilePaths = (await fastGlob(`*/${packageName}.json`, {
    cwd: opts.cacheDir,
  })).sort()
  const cafsDir = path.join(opts.storeDir, 'files')
  const metaFilesByPath: Record<string, CachedVersions> = {}
  for (const filePath of metaFilePaths) {
    const metaObject = JSON.parse(fs.readFileSync(path.join(opts.cacheDir, filePath), 'utf8')) as PackageMeta
    const cachedVersions: string[] = []
    const nonCachedVersions: string[] = []
    for (const [version, manifest] of Object.entries(metaObject.versions)) {
      if (!manifest.dist.integrity) continue
      const indexFilePath = getIndexFilePathInCafs(cafsDir, manifest.dist.integrity)
      if (fs.existsSync(indexFilePath)) {
        cachedVersions.push(version)
      } else {
        nonCachedVersions.push(version)
      }
    }
    metaFilesByPath[filePath] = {
      cachedVersions,
      nonCachedVersions,
      cachedAt: metaObject.cachedAt ? new Date(metaObject.cachedAt).toString() : undefined,
      distTags: metaObject['dist-tags'],
    }
  }
  return JSON.stringify(metaFilesByPath, null, 2)
}
