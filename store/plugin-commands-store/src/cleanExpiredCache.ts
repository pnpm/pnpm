import { type Dirent } from 'fs'
import fs from 'fs/promises'
import path from 'path'
import util from 'util'

export async function cleanExpiredCache (opts: {
  cacheDir: string
  dlxCacheMaxAge: number
  now: Date
}): Promise<void> {
  const { cacheDir, dlxCacheMaxAge, now } = opts
  const dlxCacheDir = path.join(cacheDir, 'dlx')

  if (dlxCacheMaxAge === Infinity) return

  let children: Dirent[]
  try {
    children = await fs.readdir(dlxCacheDir, {
      withFileTypes: true,
      encoding: 'utf-8',
    })
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }

  const symlinks = children.filter(item => item.isSymbolicLink())
  const directories = children.filter(item => item.isDirectory())

  await Promise.all(children.map(async (item) => {
    const dlxCachePath = path.join(dlxCacheDir, item.name)
    let shouldClean: boolean
    if (dlxCacheMaxAge <= 0) {
      shouldClean = true
    } else {
      const cacheStats = await fs.stat(dlxCachePath)
      shouldClean = cacheStats.mtime.getTime() + dlxCacheMaxAge * 60000 <= now.getTime()
    }

    if (shouldClean) {
      try {
        await Promise.all([
          fs.unlink(dlxCachePath),
          fs.realpath(dlxCachePath, { encoding: 'utf-8' })
            .then(realCachePath => fs.rm(realCachePath, { recursive: true })),
        ])
      } catch { }
    }
  }))

  await cleanOrphans({
    dlxCacheDir,
    symlinks,
    directories,
  })
}

async function cleanOrphans (opts: {
  dlxCacheDir: string
  symlinks: Dirent[]
  directories: Dirent[]
}): Promise<void> {
  const { dlxCacheDir, symlinks, directories } = opts
  const realPaths: string[] = []
  await Promise.all(symlinks.map(async linkDirent => {
    const linkPath = path.join(dlxCacheDir, linkDirent.name)
    let currentRealPath: string
    try {
      currentRealPath = await fs.realpath(linkPath)
    } catch (err) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
        return
      }
      throw err
    }
    if (path.dirname(currentRealPath) === dlxCacheDir) {
      realPaths.push(currentRealPath)
    }
  }))
  const orphans = directories
    .map(dirDirent => path.resolve(dlxCacheDir, dirDirent.name))
    .filter(dirPath => !realPaths.includes(dirPath))
  await Promise.all(orphans.map(async orphanPath => {
    try {
      await fs.rm(orphanPath, { recursive: true })
    } catch { }
  }))
}
