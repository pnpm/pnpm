import fs from 'fs'
import path from 'path'
import fastGlob from 'fast-glob'
import { getIndexFilePathInCafs } from '@pnpm/store.cafs'
import { type PackageMeta } from '@pnpm/npm-resolver'

type CachedVersions = Record<string, string | false>

export async function cacheViewCmd (opts: { cacheDir: string, storeDir: string }, packageName: string): Promise<string> {
  const metaFilePaths = (await fastGlob(`*/${packageName}.json`, {
    cwd: opts.cacheDir,
  })).sort()
  const cafsDir = path.join(opts.storeDir, 'files')
  const metaFilesByPath: Record<string, { versions: CachedVersions }> = {}
  for (const filePath of metaFilePaths) {
    const metaObject = JSON.parse(fs.readFileSync(path.join(opts.cacheDir, filePath), 'utf8')) as PackageMeta
    const versions: CachedVersions = {}
    for (const [version, manifest] of Object.entries(metaObject.versions)) {
      if (!manifest.dist.integrity) continue
      const indexFilePath = getIndexFilePathInCafs(cafsDir, manifest.dist.integrity)
      versions[version] = fs.existsSync(indexFilePath) ? indexFilePath : false
    }
    metaFilesByPath[filePath] = {
      versions,
    }
  }
  return JSON.stringify(metaFilesByPath, null, 2)
}
