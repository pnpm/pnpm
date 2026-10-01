import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'

import { linkExePlatformBinary } from './linkExePlatformBinary.js'

export async function linkPnpmBins (installDir: string, binDir: string, pkgName: string): Promise<void> {
  linkExePlatformBinary(installDir, pkgName)
  await linkBins(path.join(installDir, 'node_modules'), binDir, { warn: () => {}, force: true })
}
