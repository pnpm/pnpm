import { readdirSync, type Stats } from 'fs'
import fs from 'fs/promises'
import path from 'path'
import util from 'util'

export async function cleanExpiredDlxCache ({
  cacheDir,
  dlxCacheMaxAge,
  now,
}: {
  cacheDir: string
  dlxCacheMaxAge: number
  now: Date
}): Promise<void> {
  if (dlxCacheMaxAge === Infinity) return

  const dlxCacheDir = path.join(cacheDir, 'dlx')
  const dlxCacheNames = readOptDir(dlxCacheDir)
  if (!dlxCacheNames) return

  await Promise.all(dlxCacheNames.map(async (dlxCacheName) => {
    const dlxCachePath = path.join(dlxCacheDir, dlxCacheName)
    const dlxCacheLink = path.join(dlxCachePath, 'pkg')
    let shouldClean: boolean
    if (dlxCacheMaxAge <= 0) {
      shouldClean = true
    } else {
      const dlxCacheLinkStats = await getStats(dlxCacheLink)
      shouldClean = dlxCacheLinkStats !== 'ENOENT' && isOutdated(dlxCacheLinkStats, dlxCacheMaxAge, now)
    }
    if (shouldClean) {
      // delete the symlink, the symlink's target, and orphans (if any)
      await fs.rm(dlxCachePath, { recursive: true, force: true })
    }
  }))

  await cleanOrphans(dlxCacheDir)
}

export async function cleanOrphans (dlxCacheDir: string): Promise<void> {
  const dlxCacheNames = readOptDir(dlxCacheDir)
  if (!dlxCacheNames) return
  await Promise.all(dlxCacheNames.map(async dlxCacheName => {
    const dlxCachePath = path.join(dlxCacheDir, dlxCacheName)
    const dlxCacheLink = path.join(dlxCachePath, 'pkg')
    const dlxCacheLinkStats = await getStats(dlxCacheLink)
    if (dlxCacheLinkStats === 'ENOENT') {
      return fs.rm(dlxCachePath, { recursive: true, force: true })
    }
    const dlxCacheLinkTarget = await getRealPath(dlxCacheLink)
    const children = await fs.readdir(dlxCachePath)
    await Promise.all(children.map(async name => {
      if (name === 'pkg') return
      const fullPath = path.join(dlxCachePath, name)
      if (fullPath === dlxCacheLinkTarget) return
      await fs.rm(fullPath, { recursive: true, force: true })
    }))
  }))
}

function isOutdated (stats: Stats, dlxCacheMaxAge: number, now: Date): boolean {
  return stats.mtime.getTime() + dlxCacheMaxAge * 60_000 < now.getTime()
}

async function getStats (path: string): Promise<Stats | 'ENOENT'> {
  try {
    return await fs.lstat(path)
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return 'ENOENT'
    }
    throw err
  }
}

function readOptDir (dirPath: string): string[] | null {
  try {
    return readdirSync(dirPath, 'utf-8')
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

async function getRealPath (linkPath: string): Promise<string | null> {
  try {
    return await fs.realpath(linkPath)
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}
