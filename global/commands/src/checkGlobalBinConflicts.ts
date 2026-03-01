import fs from 'fs'
import path from 'path'
import {
  scanGlobalPackages,
  type GlobalPackageInfo,
} from '@pnpm/global.packages'
import { PnpmError } from '@pnpm/error'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type DependencyManifest } from '@pnpm/types'

export async function checkGlobalBinConflicts (opts: {
  globalDir: string
  globalBinDir: string
  newPkgs: Array<{ manifest: DependencyManifest, location: string }>
  shouldSkip: (pkg: GlobalPackageInfo) => boolean
}): Promise<void> {
  const newBinNames = new Set<string>()
  await Promise.all(
    opts.newPkgs.map(async (pkg) => {
      const bins = await getBinsFromPackageManifest(pkg.manifest, pkg.location)
      for (const bin of bins) {
        newBinNames.add(bin.name)
      }
    })
  )
  if (newBinNames.size === 0) return

  // Quick check: only investigate if a bin with the same name already exists
  const conflicting = [...newBinNames].filter(
    (name) => fs.existsSync(path.join(opts.globalBinDir, name))
  )
  if (conflicting.length === 0) return

  // Some bins already exist — find out if they belong to packages being replaced
  // (in which case it's fine) or to other packages (conflict).
  const existingPackages = scanGlobalPackages(opts.globalDir)
  for (const existingPkg of existingPackages) {
    if (opts.shouldSkip(existingPkg)) continue
    const modulesDir = path.join(existingPkg.installDir, 'node_modules')
    for (const alias of Object.keys(existingPkg.dependencies)) {
      const depDir = path.join(modulesDir, alias)
      const manifest = await safeReadPackageJsonFromDir(depDir) // eslint-disable-line no-await-in-loop
      if (!manifest) continue
      const bins = await getBinsFromPackageManifest(manifest as DependencyManifest, depDir) // eslint-disable-line no-await-in-loop
      for (const bin of bins) {
        if (conflicting.includes(bin.name)) {
          throw new PnpmError(
            'GLOBAL_BIN_CONFLICT',
            `Cannot install: binary "${bin.name}" would conflict with package "${alias}" that is already installed globally`,
            {
              hint: `Remove the conflicting package first: pnpm remove -g ${alias}`,
            }
          )
        }
      }
    }
  }
}
