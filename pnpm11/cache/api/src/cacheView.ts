import fs from 'node:fs'
import path from 'node:path'

import { decodeRegistry, encodeRegistry, loadMeta } from '@pnpm/resolving.npm-resolver'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { glob } from 'tinyglobby'

interface CachedVersions {
  cachedVersions: string[]
  nonCachedVersions: string[]
  cachedAt?: string
  distTags: Record<string, string>
}

export async function cacheView (opts: { cacheDir: string, storeDir: string, registry?: string }, packageName: string): Promise<string> {
  const prefix = opts.registry ? encodeRegistry(opts.registry) : '*'
  const metaFilePaths = (await glob(`${prefix}/${packageName}.jsonl`, {
    cwd: opts.cacheDir,
    expandDirectories: false,
  })).sort()
  const metaFilesByPath: Record<string, CachedVersions> = {}
  const storeIndex = new StoreIndex(opts.storeDir)
  try {
    const entries = await Promise.all(metaFilePaths.map(async (filePath) => {
      const fullPath = path.join(opts.cacheDir, filePath)
      // Every version is read below, so hydrate up front: the loader then
      // reports a damaged mirror as a miss instead of throwing partway
      // through, and the entry is skipped like an unreadable file.
      const metaObject = await loadMeta(fullPath, { hydrateEagerly: true })
      if (!metaObject) return null
      const mtime = statMtime(fullPath)
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
      return {
        key: decodeRegistry(registryName),
        value: {
          cachedVersions,
          nonCachedVersions,
          cachedAt: mtime?.toString(),
          distTags: metaObject['dist-tags'],
        },
      }
    }))
    for (const entry of entries) {
      if (entry != null) {
        metaFilesByPath[entry.key] = entry.value
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
