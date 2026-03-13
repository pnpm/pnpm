import fs from 'node:fs'
import path from 'node:path'

import type { PackageMeta } from '@pnpm/npm-resolver'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import getRegistryName from 'encode-registry'
import { glob } from 'tinyglobby'

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
  const storeIndex = new StoreIndex(opts.storeDir)
  try {
    for (const filePath of metaFilePaths) {
      let metaObject: PackageMeta | null
      try {
        metaObject = JSON.parse(fs.readFileSync(path.join(opts.cacheDir, filePath), 'utf8')) as PackageMeta
      } catch {
        continue
      }
      if (!metaObject) continue
      const cachedVersions: string[] = []
      const nonCachedVersions: string[] = []
      for (const [version, manifest] of Object.entries(metaObject.versions)) {
        if (!manifest.dist.integrity) continue
        const key = storeIndexKey(manifest.dist.integrity, `${manifest.name}@${manifest.version}`)
        if (storeIndex.has(key)) {
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
  } finally {
    storeIndex.close()
  }
  return JSON.stringify(metaFilesByPath, null, 2)
}
