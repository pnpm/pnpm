import fs from 'fs'
import path from 'path'
import { getCurrentPackageName } from '@pnpm/cli-meta'
import { handleGlobalAdd, type GlobalAddOptions } from '@pnpm/global.commands'
import { findGlobalPackage } from '@pnpm/global.packages'
import { linkBins } from '@pnpm/link-bins'
import { globalWarn } from '@pnpm/logger'

export interface InstallPnpmToToolsResult {
  binDir: string
  baseDir: string
  alreadyExisted: boolean
}

export type InstallPnpmToToolsOptions = GlobalAddOptions

export async function installPnpmToTools (pnpmVersion: string, opts: InstallPnpmToToolsOptions): Promise<InstallPnpmToToolsResult> {
  const currentPkgName = getCurrentPackageName()
  const globalDir = opts.globalPkgDir!

  // Check if already installed globally
  const existing = findGlobalPackage(globalDir, currentPkgName)
  if (existing) {
    const pkgJsonPath = path.join(existing.installDir, 'node_modules', currentPkgName, 'package.json')
    try {
      const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
      if (pkgJson.version === pnpmVersion) {
        // Re-link bins so the correct version is active in pnpmHomeDir
        await linkBins(path.join(existing.installDir, 'node_modules'), opts.bin!, { warn: globalWarn })
        return { alreadyExisted: true, baseDir: existing.installDir, binDir: opts.bin! }
      }
    } catch {}
  }

  await handleGlobalAdd(opts, [`${currentPkgName}@${pnpmVersion}`])

  const installed = findGlobalPackage(globalDir, currentPkgName)
  if (!installed) {
    throw new Error(`Failed to install ${currentPkgName}@${pnpmVersion}`)
  }

  return {
    alreadyExisted: false,
    baseDir: installed.installDir,
    binDir: opts.bin!,
  }
}
