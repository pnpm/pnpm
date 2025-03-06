import fs from 'fs'
import path from 'path'
import { glob } from 'tinyglobby'
import { getIndexFilePathInCafs } from '@pnpm/store.cafs'
import { type PackageMeta } from '@pnpm/npm-resolver'
import getRegistryName from 'encode-registry'

interface CachedVersions {
  cachedVersions: string[]
  nonCachedVersions: string[]
  cachedAt?: string
  distTags: Record<string, string>
}

export async function cacheView (opts: { cacheDir: string, storeDir: string, registry?: string }, packageName: string): Promise<string> {
  const prefix = opts.registry ? `${getRegistryName(opts.registry)}` : '*'
  const metaFilePaths = (await glob(`${prefix}/${packageName}.json`, {
    cwd: opts.cacheDir,
    expandDirectories: false,
  })).sort()
  const metaFilesByPath: Record<string, CachedVersions> = {}
  for (const filePath of metaFilePaths) {
    const metaObject = JSON.parse(fs.readFileSync(path.join(opts.cacheDir, filePath), 'utf8')) as PackageMeta
    const cachedVersions: string[] = []
    const nonCachedVersions: string[] = []
    for (const [version, manifest] of Object.entries(metaObject.versions)) {
      if (!manifest.dist.integrity) continue
      const indexFilePath = getIndexFilePathInCafs(opts.storeDir, manifest.dist.integrity, `${manifest.name}@${manifest.version}`)
      if (fs.existsSync(indexFilePath)) {
        cachedVersions.push(version)
      } else {
        nonCachedVersions.push(version)
      }
    }
    let registryName = filePath
    while (path.dirname(registryName) !== '.') {
      registryName = path.dirname(registryName)
    }
    metaFilesByPath[registryName.replaceAll('+', ':')] = {
      cachedVersions,
      nonCachedVersions,
      cachedAt: metaObject.cachedAt ? new Date(metaObject.cachedAt).toString() : undefined,
      distTags: metaObject['dist-tags'],
    }
  }
  return JSON.stringify(metaFilesByPath, null, 2)
}
