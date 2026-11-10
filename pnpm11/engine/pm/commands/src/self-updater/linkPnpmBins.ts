import fs from 'node:fs'
import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import { createStagingToken } from '@pnpm/building.during-install'
import { packageManager } from '@pnpm/cli.meta'
import { isError } from '@pnpm/error'
import { renameFileWithRetryAsync } from '@pnpm/fs.graceful-fs'

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

/**
 * Links a store slot's bins unless this pnpm already did. Concurrent commands
 * may link the same slot: each links the bins into a private directory beside
 * `binDir` and renames them into `binDir` one by one, so a shim in `binDir` is
 * always complete. On Windows, concurrent renames onto one path fail
 * transiently, so they are retried. The marker is written last.
 */
export async function ensureStoreBinsLinked (pnpmGvsPath: string, binDir: string, pkgName: string): Promise<void> {
  if (areStoreBinsCurrent(binDir)) return
  const token = createStagingToken()
  const privateBinDir = path.join(path.dirname(binDir), `.${path.basename(binDir)}-${token}`)
  try {
    await linkPnpmBins(pnpmGvsPath, privateBinDir, pkgName)
    await fs.promises.mkdir(binDir, { recursive: true })
    await Promise.all((await fs.promises.readdir(privateBinDir)).map(async (binName) =>
      renameFileWithRetryAsync(path.join(privateBinDir, binName), path.join(binDir, binName))
    ))
    const markerTemp = path.join(binDir, `${STORE_BINS_MARKER}-${token}`)
    await fs.promises.writeFile(markerTemp, packageManager.version)
    await renameFileWithRetryAsync(markerTemp, path.join(binDir, STORE_BINS_MARKER))
  } finally {
    await fs.promises.rm(privateBinDir, { recursive: true, force: true })
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
