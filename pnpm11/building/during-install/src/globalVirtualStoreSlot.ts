import fs from 'node:fs/promises'
import path from 'node:path'

import { buildsDependenciesDuringInstall, getStagingDir } from '@pnpm/deps.graph-hasher'
import { isError } from '@pnpm/error'
import type { AllowBuild, DepPath } from '@pnpm/types'
import { pathExists } from 'path-exists'

/**
 * A global virtual store slot's package directory is final once its
 * `package.json` exists: nothing writes into it afterwards, so installs share
 * slots without locks. An import writes `package.json` last. A package that an
 * install builds is built in a staging directory beside the package directory
 * and renamed into place once its build has finished.
 */
export async function isPackagePlaced (pkgDir: string): Promise<boolean> {
  return pathExists(path.join(pkgDir, 'package.json'))
}

let stagingTokenCount = 0

/** A token that is unique to one install in this process. It names the install's staging directories. */
export function createStagingToken (): string {
  return `${process.pid.toString(16)}-${(stagingTokenCount++).toString(16)}`
}

export interface SlotImportOptions {
  allowBuild?: AllowBuild
  deferDependencyBuilds?: boolean
  enableGlobalVirtualStore?: boolean
  ignoreScripts?: boolean
  /** See {@link createStagingToken}. Set under the global virtual store only. */
  stagingToken?: string
}

/**
 * The directory the link phase imports a package into: its staging directory
 * when this install builds it into a global virtual store slot and has no
 * cached build to import, and its package directory otherwise. Resolves to
 * `undefined` when the slot already holds such a package, which needs no
 * import.
 */
export async function selectImportDir (
  pkg: { depPath: DepPath, dir: string, patch?: unknown, requiresBuild?: boolean },
  { hasCachedBuild, opts }: { hasCachedBuild: boolean, opts: SlotImportOptions }
): Promise<string | undefined> {
  if (opts.stagingToken == null || !buildsInThisInstall(pkg, opts)) return pkg.dir
  if (await isPackagePlaced(pkg.dir)) return undefined
  return hasCachedBuild ? pkg.dir : getStagingDir(pkg.dir, opts.stagingToken)
}

function buildsInThisInstall (
  pkg: { depPath: DepPath, patch?: unknown, requiresBuild?: boolean },
  opts: SlotImportOptions
): boolean {
  return pkg.patch != null ||
    (pkg.requiresBuild === true && opts.allowBuild?.(pkg.depPath) === true && buildsDependenciesDuringInstall(opts))
}

const MAX_PLACE_ATTEMPTS = 3

/**
 * Renames a built staging directory onto its package directory. Resolves once
 * the package directory holds the package, placed by this install or by a
 * concurrent one, and the staging directory is gone. A package directory
 * without `package.json` is an unfinished import, which is moved aside.
 */
export async function placeBuiltPackage (stagingDir: string, pkgDir: string): Promise<void> {
  for (let attempt = 1; ; attempt++) {
    try {
      // eslint-disable-next-line no-await-in-loop -- each attempt depends on the outcome of the previous one
      await fs.rename(stagingDir, pkgDir)
      return
    } catch (err: unknown) {
      // eslint-disable-next-line no-await-in-loop -- see above
      if (await resolvePlacementConflict({ attempt, err, pkgDir, stagingDir }) === 'placed') return
    }
  }
}

/**
 * Handles a failed rename of `stagingDir` onto `pkgDir`. Resolves to `placed`
 * when another install placed the package, and to `retry` once an unfinished
 * import has been moved aside. Rethrows `err` otherwise.
 */
async function resolvePlacementConflict (
  { attempt, err, pkgDir, stagingDir }: { attempt: number, err: unknown, pkgDir: string, stagingDir: string }
): Promise<'placed' | 'retry'> {
  if (!await pathExists(pkgDir)) throw err
  if (await isPackagePlaced(pkgDir)) {
    await removeDir(stagingDir)
    return 'placed'
  }
  if (attempt === MAX_PLACE_ATTEMPTS) throw err
  await moveAside(pkgDir, `${stagingDir}-stale-${attempt}`)
  return 'retry'
}

/**
 * Replaces a placed package with a rebuilt one. The placed directory is moved
 * aside, the staging directory is renamed in, and the old directory is removed.
 */
export async function replacePlacedPackage (stagingDir: string, pkgDir: string): Promise<void> {
  const previousDir = `${stagingDir}-previous`
  await moveAside(pkgDir, previousDir)
  await placeBuiltPackage(stagingDir, pkgDir)
}

async function moveAside (dir: string, asideDir: string): Promise<void> {
  try {
    await fs.rename(dir, asideDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
  await removeDir(asideDir)
}

export async function removeDir (dir: string): Promise<void> {
  await fs.rm(dir, { recursive: true, force: true })
}
