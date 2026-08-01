import fs from 'node:fs'
import path from 'node:path'

import { loadMeta } from '@pnpm/resolving.npm-resolver'
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
  const metaFilePaths = (await glob(`${prefix}/${packageName}.jsonl`, {
    cwd: opts.cacheDir,
    expandDirectories: false,
  })).sort()
  const cachedMetaFiles = await Promise.all(metaFilePaths.map(async (filePath) => {
    const fullPath = path.join(opts.cacheDir, filePath)
    // Every version is read below, so hydrate up front: the loader then
    // reports a damaged mirror as a miss instead of throwing partway
    // through, and the entry is skipped like an unreadable file.
    const meta = await loadMeta(fullPath, { hydrateEagerly: true })
    return { filePath, meta, mtime: statMtime(fullPath) }
  }))
  const metaFilesByPath: Record<string, CachedVersions> = {}
  const storeIndex = new StoreIndex(opts.storeDir)
  try {
    for (const { filePath, meta: metaObject, mtime } of cachedMetaFiles) {
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
        cachedAt: mtime?.toString(),
        distTags: metaObject['dist-tags'],
      }
    }
  } finally {
    storeIndex.close()
  }
  return JSON.stringify(metaFilesByPath, null, 2)
}

function statMtime (filePath: string): Date | undefined {
  try {
    return fs.statSync(filePath).mtime
  } catch {
    return undefined
  }
}
