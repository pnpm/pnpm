import fs from 'fs/promises'
import path from 'path'
import util from 'util'
import { streamParser } from '@pnpm/logger'
import { type StoreController } from '@pnpm/store-controller-types'
import { type ReporterFunction } from './types'

export async function storePrune (
  opts: {
    reporter?: ReporterFunction
    storeController: StoreController
    removeAlienFiles?: boolean
    cacheDir: string
    dlxCacheMaxAge: number
  }
) {
  const reporter = opts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  await opts.storeController.prune(opts.removeAlienFiles)
  await opts.storeController.close()

  await cleanExpiredCache({
    cacheDir: opts.cacheDir,
    dlxCacheMaxAge: opts.dlxCacheMaxAge,
    now: new Date(),
  })

  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }
}

export async function cleanExpiredCache (opts: {
  cacheDir: string
  dlxCacheMaxAge: number
  now: Date
}): Promise<void> {
  const { cacheDir, dlxCacheMaxAge, now } = opts
  const dlxCacheDir = path.join(cacheDir, 'dlx')

  if (dlxCacheMaxAge === Infinity) return

  let cacheNames: string[]
  try {
    cacheNames = await fs.readdir(dlxCacheDir, { encoding: 'utf-8' })
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
  await Promise.all(cacheNames.map(async name => {
    const dlxCachePath = path.join(dlxCacheDir, name)
    let shouldClean: boolean
    if (dlxCacheMaxAge <= 0) {
      shouldClean = true
    } else {
      const cacheStats = await fs.stat(dlxCachePath)
      shouldClean = cacheStats.mtime.getTime() + dlxCacheMaxAge * 60_000 <= now.getTime()
    }
    if (shouldClean) {
      try {
        await fs.rm(dlxCachePath, { recursive: true })
      } catch {}
    }
  }))
}
