import fs from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import {
  findGlobalPackage,
  getHashLink,
  getInstalledBinNames,
  type GlobalPackageInfo,
} from '@pnpm/global.packages'
import { removeBin } from '@pnpm/remove-bins'
import isSubdir from 'is-subdir'

export async function handleGlobalRemove (
  opts: {
    globalPkgDir?: string
    bin?: string
  },
  params: string[]
): Promise<void> {
  const globalDir = opts.globalPkgDir!
  const globalBinDir = opts.bin!

  // Find all groups that contain the packages to remove (dedup by hash)
  const groupsToRemove = new Map<string, GlobalPackageInfo>()
  for (const param of params) {
    const pkg = findGlobalPackage(globalDir, param)
    if (!pkg) {
      throw new PnpmError('GLOBAL_PKG_NOT_FOUND', `Cannot remove '${param}': not found in global packages`)
    }
    groupsToRemove.set(pkg.hash, pkg)
  }

  // Remove bins, hash symlinks, and install dirs for all affected groups in parallel
  await Promise.all(
    [...groupsToRemove.entries()].map(async ([hash, pkg]) => {
      const binNames = await getInstalledBinNames(pkg)
      await Promise.all(binNames.map((binName) => removeBin(path.join(globalBinDir, binName))))
      await fs.promises.rm(getHashLink(globalDir, hash), { force: true })
      if (isSubdir(globalDir, pkg.installDir)) {
        await fs.promises.rm(pkg.installDir, { recursive: true, force: true })
      }
    })
  )
}
