import fs from 'node:fs'
import path from 'node:path'

import { getBinsFromPackageManifest, pkgOwnsBin } from '@pnpm/bins.resolver'
import { PnpmError } from '@pnpm/error'
import {
  type GlobalPackageInfo,
  scanGlobalPackages,
} from '@pnpm/global.packages'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DependencyManifest } from '@pnpm/types'

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

  const newBinOwners = await mapBinsToOwners(opts.newPkgs)
  if (newBinOwners.size === 0) return binsToSkip

  const conflicting = new Set(
    [...newBinOwners.keys()].filter((name) => binSlotExists(opts.globalBinDir, name))
  )
  if (conflicting.size === 0) return binsToSkip

  // Some bins already exist — find out if they belong to packages being replaced
  // (in which case it's fine) or to other packages (conflict).
  const existingPackages = scanGlobalPackages(opts.globalDir)
  for (const existingPkg of existingPackages) {
    if (opts.shouldSkip(existingPkg)) continue
    // eslint-disable-next-line no-await-in-loop -- the scan stops at the first conflict, so later packages are not read
    await collectBinsToSkipFromExistingPackage({ existingPkg, conflicting, newBinOwners, binsToSkip })
  }
  return binsToSkip
}

interface ExistingPackageBinCheck {
  existingPkg: GlobalPackageInfo
  conflicting: Set<string>
  newBinOwners: Map<string, string[]>
  binsToSkip: Set<string>
}

/** Maps each new bin name to all packages that provide it. */
async function mapBinsToOwners (
  newPkgs: Array<{ manifest: DependencyManifest, location: string }>
): Promise<Map<string, string[]>> {
  const newBinOwners = new Map<string, string[]>()
  await Promise.all(
    newPkgs.map(async (pkg) => {
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
  return newBinOwners
}

async function collectBinsToSkipFromExistingPackage (check: ExistingPackageBinCheck): Promise<void> {
  const modulesDir = path.join(check.existingPkg.installDir, 'node_modules')
  for (const alias of Object.keys(check.existingPkg.dependencies)) {
    const depDir = path.join(modulesDir, alias)
    const manifest = await safeReadPackageJsonFromDir(depDir) // eslint-disable-line no-await-in-loop -- the scan stops at the first conflict, so later packages are not read
    if (!manifest) continue
    const bins = await getBinsFromPackageManifest(manifest as DependencyManifest, depDir) // eslint-disable-line no-await-in-loop -- the scan stops at the first conflict, so later packages are not read
    for (const bin of bins) {
      if (!check.conflicting.has(bin.name)) continue
      resolveBinConflict(check, { binName: bin.name, alias, existingPkgName: manifest.name })
    }
  }
}

function resolveBinConflict (
  check: ExistingPackageBinCheck,
  existing: { binName: string, alias: string, existingPkgName: string }
): void {
  const { binName, alias, existingPkgName } = existing
  const newOwns = check.newBinOwners.get(binName)!.some((owner) => pkgOwnsBin(binName, owner))
  const existingOwns = pkgOwnsBin(binName, existingPkgName)
  // If only the new package owns this bin, it gets priority and is
  // allowed to override the existing bin.
  if (newOwns && !existingOwns) return
  // If only the existing package owns this bin, the new package
  // should skip linking it rather than failing the entire install.
  if (existingOwns && !newOwns) {
    check.binsToSkip.add(binName)
    return
  }
  // If both own it (e.g. "pnpm" vs "@pnpm/exe"), or neither owns
  // it, fall through to the conflict error below.
  const conflictDisplay = alias === existingPkgName
    ? `"${alias}"`
    : `"${alias}" (package "${existingPkgName}")`
  throw new PnpmError(
    'GLOBAL_BIN_CONFLICT',
    `Cannot install: binary "${binName}" would conflict with ${conflictDisplay} that is already installed globally`,
    {
      hint: `Remove the conflicting package first: pnpm remove -g ${alias}`,
    }
  )
}

// Whether a bin named `name` already occupies a slot in `globalBinDir`. On
// Windows the `node` runtime bin is linked as `<name>.exe` (with no bare
// `<name>` file), so that flavor is checked too — otherwise an existing
// `node.exe` would not be detected as a conflict and could be silently
// overwritten.
function binSlotExists (globalBinDir: string, name: string): boolean {
  if (fs.existsSync(path.join(globalBinDir, name))) return true
  return process.platform === 'win32' && fs.existsSync(path.join(globalBinDir, `${name}.exe`))
}
