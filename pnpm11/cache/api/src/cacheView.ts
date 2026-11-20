import fs from 'node:fs'
import path from 'node:path'

import { decodeRegistry, encodeRegistry, loadMeta, type PackageMeta } from '@pnpm/resolving.npm-resolver'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { glob } from 'tinyglobby'

interface CachedVersions {
  cachedVersions: string[]
  nonCachedVersions: string[]
  cachedAt?: string
  distTags: Record<string, string>
}

interface CachedMetaFile {
  metaObject: PackageMeta
  mtime: Date
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
    for (const filePath of metaFilePaths) {
      // eslint-disable-next-line no-await-in-loop -- sequential, so only one packument is held in memory at a time
      const metaFile = await readCachedMetaFile(path.join(opts.cacheDir, filePath))
      if (metaFile == null) continue
      const { metaObject, mtime } = metaFile
      metaFilesByPath[decodeRegistry(getTopLevelDir(filePath))] = {
        ...partitionVersionsByStorePresence(storeIndex, metaObject),
        cachedAt: mtime.toString(),
        distTags: metaObject['dist-tags'],
      }
    }
  } finally {
    storeIndex.close()
  }
  return JSON.stringify(metaFilesByPath, null, 2)
}

async function readCachedMetaFile (fullPath: string): Promise<CachedMetaFile | undefined> {
  // Every version is read, so hydrate up front: the loader then reports a
  // damaged mirror as a miss instead of throwing partway through, and the
  // file is skipped like an unreadable one.
  const metaObject = await loadMeta(fullPath, { hydrateEagerly: true })
  if (metaObject == null) return undefined
  try {
    return { metaObject, mtime: fs.statSync(fullPath).mtime }
  } catch {
    return undefined
  }
}

function partitionVersionsByStorePresence (
  storeIndex: StoreIndex,
  metaObject: PackageMeta
): Pick<CachedVersions, 'cachedVersions' | 'nonCachedVersions'> {
  const cachedVersions: string[] = []
  const nonCachedVersions: string[] = []
  for (const [version, manifest] of Object.entries(metaObject.versions)) {
    // A version without a manifest (a fragment of the wrong shape) is skipped.
    if (!manifest?.dist?.integrity) continue
    const key = storeIndexKey(manifest.dist.integrity, `${manifest.name}@${manifest.version}`)
    if (storeIndex.has(key)) {
      cachedVersions.push(version)
    } else {
      nonCachedVersions.push(version)
    }
  }
  return { cachedVersions, nonCachedVersions }
}

function getTopLevelDir (filePath: string): string {
  let topLevelDir = filePath
  while (path.dirname(topLevelDir) !== '.') {
    topLevelDir = path.dirname(topLevelDir)
  }
  return topLevelDir
}
