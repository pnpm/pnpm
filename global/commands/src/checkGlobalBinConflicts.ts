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

// Bins that are logically owned by a package other than the one matching
// the bin name.  For example, `npx` ships inside the `npm` package, so
// when `npm` is being installed globally it should be allowed to claim
// the `npx` bin as well.
const BIN_OWNER_OVERRIDES: Record<string, string> = {
  npx: 'npm',
}

function pkgOwnsBin (binName: string, pkgName: string): boolean {
  return binName === pkgName || BIN_OWNER_OVERRIDES[binName] === pkgName
}

/**
 * Checks for bin name conflicts between new packages and existing global
 * packages.  Returns a set of bin names that should be skipped during linking
 * because they are legitimately owned by an already-installed package.
 */
export async function checkGlobalBinConflicts (opts: {
  globalDir: string
  globalBinDir: string
  newPkgs: Array<{ manifest: DependencyManifest, location: string }>
  shouldSkip: (pkg: GlobalPackageInfo) => boolean
}): Promise<Set<string>> {
  const binsToSkip = new Set<string>()

  // Map each new bin name to all packages that provide it
  const newBinOwners = new Map<string, string[]>()
  await Promise.all(
    opts.newPkgs.map(async (pkg) => {
      const bins = await getBinsFromPackageManifest(pkg.manifest, pkg.location)
      for (const bin of bins) {
        const owners = newBinOwners.get(bin.name)
        if (owners) {
          owners.push(pkg.manifest.name)
        } else {
          newBinOwners.set(bin.name, [pkg.manifest.name])
        }
      }
    })
  )
  if (newBinOwners.size === 0) return binsToSkip

  // Quick check: only investigate if a bin with the same name already exists
  const conflicting = new Set(
    [...newBinOwners.keys()].filter(
      (name) => fs.existsSync(path.join(opts.globalBinDir, name))
    )
  )
  if (conflicting.size === 0) return binsToSkip

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
        if (!conflicting.has(bin.name)) continue
        // If any new package owns this bin (name match or override), it
        // gets priority and is allowed to override the existing bin.
        if (newBinOwners.get(bin.name)!.some((owner) => pkgOwnsBin(bin.name, owner))) continue
        // If the existing package owns this bin, the new package should
        // skip linking it rather than failing the entire install.
        if (pkgOwnsBin(bin.name, manifest.name)) {
          binsToSkip.add(bin.name)
          continue
        }
        const conflictDisplay = alias === manifest.name
          ? `"${alias}"`
          : `"${alias}" (package "${manifest.name}")`
        throw new PnpmError(
          'GLOBAL_BIN_CONFLICT',
          `Cannot install: binary "${bin.name}" would conflict with ${conflictDisplay} that is already installed globally`,
          {
            hint: `Remove the conflicting package first: pnpm remove -g ${alias}`,
          }
        )
      }
    }
  }
  return binsToSkip
}
