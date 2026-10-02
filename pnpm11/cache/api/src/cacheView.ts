import fs from 'node:fs'
import path from 'node:path'

import { decodeRegistry, encodeRegistry, type PackageMeta } from '@pnpm/resolving.npm-resolver'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { glob } from 'tinyglobby'

interface CachedVersions {
  cachedVersions: string[]
  nonCachedVersions: string[]
  cachedAt?: string
  distTags: Record<string, string>
}

interface CachedMetaFile {
  metaObject: PackageMeta | null
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
      const metaFile = readCachedMetaFile(path.join(opts.cacheDir, filePath))
      if (!metaFile?.metaObject) continue
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

function readCachedMetaFile (fullPath: string): CachedMetaFile | undefined {
  try {
    const raw = fs.readFileSync(fullPath, 'utf8')
    const mtime = fs.statSync(fullPath).mtime
    return { metaObject: parseCachedMeta(raw), mtime }
  } catch {
    return undefined
  }
}

function parseCachedMeta (raw: string): PackageMeta | null {
  const newlineIdx = raw.indexOf('\n')
  if (newlineIdx !== -1) {
    // NDJSON format: line 1 = headers, line 2 = metadata
    return JSON.parse(raw.slice(newlineIdx + 1)) as PackageMeta
  }
  return JSON.parse(raw) as PackageMeta
}

function partitionVersionsByStorePresence (
  storeIndex: StoreIndex,
  metaObject: PackageMeta
): Pick<CachedVersions, 'cachedVersions' | 'nonCachedVersions'> {
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
  return { cachedVersions, nonCachedVersions }
}

function getTopLevelDir (filePath: string): string {
  let topLevelDir = filePath
  while (path.dirname(topLevelDir) !== '.') {
    topLevelDir = path.dirname(topLevelDir)
  }
  return topLevelDir
}
