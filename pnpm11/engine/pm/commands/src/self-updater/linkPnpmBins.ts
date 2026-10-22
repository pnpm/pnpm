import fs from 'node:fs'
import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import { lockGlobalVirtualStoreSlot } from '@pnpm/building.during-install'
import { packageManager } from '@pnpm/cli.meta'
import { isError } from '@pnpm/error'

import { linkExePlatformBinary } from './linkExePlatformBinary.js'

export async function linkPnpmBins (installDir: string, binDir: string, pkgName: string): Promise<void> {
  linkExePlatformBinary(installDir, pkgName)
  await linkBins(path.join(installDir, 'node_modules'), binDir, { warn: () => {}, force: true })
}

/**
 * Holds the version of the pnpm that linked a store slot's bins. A slot without
 * this pnpm's marker is relinked, which repairs shims linked by older releases
 * (https://github.com/pnpm/pnpm/issues/16646).
 */
const STORE_BINS_MARKER = '.pnpm-bins-linked'

/** Links a store slot's bins unless this pnpm already did, under the slot's lock when it can be taken. */
export async function ensureStoreBinsLinked (pnpmGvsPath: string, binDir: string, pkgName: string): Promise<void> {
  if (areStoreBinsCurrent(binDir)) return
  const lock = await lockGlobalVirtualStoreSlot(path.join(pnpmGvsPath, 'node_modules'))
  try {
    if (areStoreBinsCurrent(binDir)) return
    await linkPnpmBins(pnpmGvsPath, binDir, pkgName)
    // An unlocked link may have raced another one, so it is redone next time.
    if (lock != null) {
      fs.writeFileSync(path.join(binDir, STORE_BINS_MARKER), packageManager.version)
    }
  } finally {
    await lock?.release()
  }
}

function areStoreBinsCurrent (binDir: string): boolean {
  try {
    return fs.readFileSync(path.join(binDir, STORE_BINS_MARKER), 'utf8') === packageManager.version
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}
