import fs from 'node:fs'
import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import { packageManager } from '@pnpm/cli.meta'
import { isError } from '@pnpm/error'

import { linkExePlatformBinary } from './linkExePlatformBinary.js'

export async function linkPnpmBins (installDir: string, binDir: string, pkgName: string): Promise<void> {
  linkExePlatformBinary(installDir, pkgName)
  await linkBins(path.join(installDir, 'node_modules'), binDir, { warn: () => {}, force: true })
}

/**
 * Written to a store slot's bin directory after its bins are linked, holding
 * the version of the pnpm that linked them. Releases older than 11.28.4 left
 * shims that run `node` on the pnpm v12 native binary
 * (https://github.com/pnpm/pnpm/issues/16646), so a slot without this pnpm's
 * marker is relinked.
 */
const STORE_BINS_MARKER = '.pnpm-bins-linked'

export function areStoreBinsCurrent (binDir: string): boolean {
  try {
    return fs.readFileSync(path.join(binDir, STORE_BINS_MARKER), 'utf8') === packageManager.version
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}

export async function linkStoreBins (pnpmGvsPath: string, binDir: string, pkgName: string): Promise<void> {
  await linkPnpmBins(pnpmGvsPath, binDir, pkgName)
  fs.writeFileSync(path.join(binDir, STORE_BINS_MARKER), packageManager.version)
}
