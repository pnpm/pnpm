import fs from 'node:fs'
import path from 'node:path'

import type { PackageMeta } from '@pnpm/resolving.npm-resolver'
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
      const fullPath = path.join(opts.cacheDir, filePath)
      let mtime: Date | undefined
      try {
        const raw = fs.readFileSync(fullPath, 'utf8')
        mtime = fs.statSync(fullPath).mtime
        const newlineIdx = raw.indexOf('\n')
        if (newlineIdx !== -1) {
          // NDJSON format: line 1 = headers, line 2 = metadata
          metaObject = JSON.parse(raw.slice(newlineIdx + 1)) as PackageMeta
        } else {
          metaObject = JSON.parse(raw) as PackageMeta
        }
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
        cachedAt: mtime?.toString(),
        distTags: metaObject['dist-tags'],
      }
    }
  } finally {
    storeIndex.close()
  }
  return JSON.stringify(metaFilesByPath, null, 2)
}
