import { randomBytes } from 'node:crypto'
import fs from 'node:fs/promises'
import path from 'node:path'

import { buildsDependenciesDuringInstall, getStagingDir } from '@pnpm/deps.graph-hasher'
import { isError } from '@pnpm/error'
import { withFileLockRetryAsync } from '@pnpm/fs.graceful-fs'
import { globalWarn } from '@pnpm/logger'
import type { AllowBuild, DepPath } from '@pnpm/types'
import { pathExists } from 'path-exists'

/**
 * A global virtual store slot's package directory is placed once its
 * `package.json` exists. The package's own files never change after that, so
 * installs share slots without locks. Only the bin links of its dependencies,
 * under `node_modules/.bin`, are relinked, idempotently. An import writes
 * `package.json` last. A package that an install builds is built in a staging
 * directory beside the package directory and renamed into place once its build
 * has finished.
 */
export async function isPackagePlaced (pkgDir: string): Promise<boolean> {
  return pathExists(path.join(pkgDir, 'package.json'))
}

let stagingTokenCount = 0

/**
 * A token that is unique to one install. It names the install's staging
 * directories. The random part keeps installs in different containers or on
 * different hosts apart when they share a store and a process id.
 */
export function createStagingToken (): string {
  return `${process.pid.toString(16)}-${(stagingTokenCount++).toString(16)}-${randomBytes(8).toString('hex')}`
}

export interface SlotImportOptions {
  allowBuild?: AllowBuild
  deferDependencyBuilds?: boolean
  enableGlobalVirtualStore?: boolean
  ignoreScripts?: boolean
  /** See {@link createStagingToken}. Set under the global virtual store only. */
  stagingToken?: string
}

interface SlotPackage {
  depPath: DepPath
  dir: string
  patch?: unknown
  requiresBuild?: boolean
  resolution?: unknown
}

/**
 * The directory the link phase imports a package into: its staging directory
 * when this install builds it into a global virtual store slot and has no
 * cached build to import, and its package directory otherwise. Resolves to
 * `undefined` when the slot already holds such a package, which needs no
 * import. A package from a local directory is staged even then, since its
 * files may have changed: its build replaces the placed package.
 */
export async function selectImportDir (
  pkg: SlotPackage,
  { hasCachedBuild, opts }: { hasCachedBuild: boolean, opts: SlotImportOptions }
): Promise<string | undefined> {
  if (opts.stagingToken == null || !buildsInThisInstall(pkg, opts)) return pkg.dir
  const stagingDir = getStagingDir(pkg.dir, opts.stagingToken)
  if (isFromLocalDirectory(pkg)) return stagingDir
  if (await isPackagePlaced(pkg.dir)) return undefined
  return hasCachedBuild ? pkg.dir : stagingDir
}

/** Whether a package was resolved from a local directory, such as a `file:` directory or an injected workspace package. */
export function isFromLocalDirectory (pkg: { resolution?: unknown }): boolean {
  return typeof pkg.resolution === 'object' && pkg.resolution != null &&
    'type' in pkg.resolution && pkg.resolution.type === 'directory'
}

function buildsInThisInstall (pkg: SlotPackage, opts: SlotImportOptions): boolean {
  return pkg.patch != null ||
    (pkg.requiresBuild === true && opts.allowBuild?.(pkg.depPath) === true && buildsDependenciesDuringInstall(opts))
}

const MAX_PLACE_ATTEMPTS = 3

class TargetExistsError extends Error {}

/**
 * Renames a built staging directory onto its package directory. Resolves once
 * the package directory holds the package, placed by this install or by a
 * concurrent one. A package directory without `package.json` is an unfinished
 * import, which is moved aside. The staging directory is left for the caller
 * to discard when another install placed the package first.
 */
export async function placeBuiltPackage (stagingDir: string, pkgDir: string): Promise<'renamed' | 'placedByOther'> {
  for (let attempt = 1; ; attempt++) {
    try {
      // eslint-disable-next-line no-await-in-loop -- each attempt depends on the outcome of the previous one
      await renameOntoMissing(stagingDir, pkgDir)
      return 'renamed'
    } catch (err: unknown) {
      // eslint-disable-next-line no-await-in-loop -- see above
      if (await resolvePlacementConflict({ attempt, err, pkgDir, stagingDir }) === 'placedByOther') return 'placedByOther'
    }
  }
}

/**
 * Handles a failed rename of `stagingDir` onto `pkgDir`. Resolves to
 * `placedByOther` when another install placed the package, and to `retry` once
 * an unfinished import has been moved aside. Rethrows `err` otherwise.
 */
async function resolvePlacementConflict (
  { attempt, err, pkgDir, stagingDir }: { attempt: number, err: unknown, pkgDir: string, stagingDir: string }
): Promise<'placedByOther' | 'retry'> {
  if (!await pathExists(pkgDir)) throw err
  if (await isPackagePlaced(pkgDir)) return 'placedByOther'
  if (attempt === MAX_PLACE_ATTEMPTS) throw err
  await moveAside(pkgDir, `${stagingDir}-stale-${attempt}`)
  return 'retry'
}

/**
 * Retries a rename that a transient file lock fails, as Windows does. A target
 * that exists is a lost race, not a lock, so it ends the retries at once.
 */
async function renameOntoMissing (src: string, dest: string): Promise<void> {
  await withFileLockRetryAsync(async () => {
    if (await pathExists(dest)) throw new TargetExistsError(dest)
    await fs.rename(src, dest)
  })
}

/**
 * Replaces a placed package with a rebuilt one. The placed directory is moved
 * aside, the staging directory is renamed in, and the old directory is removed.
 */
export async function replacePlacedPackage (stagingDir: string, pkgDir: string): Promise<void> {
  await moveAside(pkgDir, `${stagingDir}-replaced`)
  if (await placeBuiltPackage(stagingDir, pkgDir) === 'placedByOther') {
    await discardStagingDir(stagingDir)
  }
}

async function moveAside (dir: string, asideDir: string): Promise<void> {
  try {
    await withFileLockRetryAsync(async () => fs.rename(dir, asideDir))
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
  await discardStagingDir(asideDir)
}

export async function removeDir (dir: string): Promise<void> {
  await fs.rm(dir, { recursive: true, force: true, maxRetries: 3 })
}

/**
 * Removes a staging directory this install no longer needs. A failure is
 * reported, not thrown: the directory holds nothing another install uses, and
 * `pnpm store prune` removes it later.
 */
export async function discardStagingDir (dir: string): Promise<void> {
  try {
    await removeDir(dir)
  } catch (err: unknown) {
    globalWarn(`Failed to remove ${dir}: ${isError(err) ? err.message : String(err)}`)
  }
}
