import path from 'node:path'

import { isError, PnpmError } from '@pnpm/error'
import { readCurrentLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject, LockfileResolution } from '@pnpm/lockfile.types'
import { packageIdFromSnapshot } from '@pnpm/lockfile.utils'
import { getFilePathByModeInCafs, type PackageFileInfo, type PackageFilesIndex } from '@pnpm/store.cafs'
import { gitHostedStoreIndexKey, pickStoreIndexKey, type StoreIndex, storeIndexKey } from '@pnpm/store.index'
import type { DepPath } from '@pnpm/types'

import { getRegisteredProjects } from './projectRegistry.js'

/**
 * Every file of the store index rows of the packages in each registered
 * project's installed lockfile, including the files of every side-effects
 * entry. A package imported by copy or clone, such as a patched or built
 * one, has no hard link to the store, so the prune needs this list to keep
 * it. A project without an installed lockfile and a package without a row
 * add nothing.
 */
export async function getInUseFiles (storeDir: string, storeIndex: StoreIndex): Promise<Set<string>> {
  const projects = await getRegisteredProjects(storeDir)
  const lockfiles = await Promise.all(projects.map(readInstalledLockfile))
  const keys = new Set(lockfiles.flatMap((lockfile) => (lockfile == null ? [] : lockfileIndexKeys(lockfile))))
  const files = new Set<string>()
  for (const key of keys) {
    const row = storeIndex.get(key) as PackageFilesIndex | undefined
    if (row == null) continue
    for (const { digest, mode } of rowFiles(row)) {
      files.add(path.normalize(getFilePathByModeInCafs(storeDir, digest, mode)))
    }
  }
  return files
}

function lockfileIndexKeys (lockfile: LockfileObject): string[] {
  return Object.entries(lockfile.packages ?? {}).flatMap(([depPath, snapshot]) =>
    packageIndexKeys(snapshot.resolution, packageIdFromSnapshot(depPath as DepPath, snapshot))
  )
}

/**
 * The lockfile the last install wrote into the project's `node_modules`,
 * which records what is installed there.
 */
async function readInstalledLockfile (projectDir: string): Promise<LockfileObject | null> {
  // A project with a custom modules directory has no lockfile here, so only
  // their hard links keep its packages.
  const installStateDir = path.join(projectDir, 'node_modules', '.pnpm')
  try {
    return await readCurrentLockfile(installStateDir, { ignoreIncompatible: false })
  } catch (err: unknown) {
    throw new PnpmError('STORE_DIR_PRUNE_IN_USE_FILES',
      `Failed to read the store files that registered projects use: ${installStateDir}: ${isError(err) ? err.message : String(err)}`)
  }
}

/**
 * The lockfile does not say which key the install used for a git-hosted
 * package, which has a key for its built and its unbuilt contents, or for
 * a variations resolution, whose variant depends on the installing host
 * and its `supportedArchitectures`.
 */
function packageIndexKeys (resolution: LockfileResolution, pkgId: string): string[] {
  if (!('type' in resolution) || resolution.type == null) {
    if ('tarball' in resolution) {
      return [true, false].map((built) => pickStoreIndexKey(resolution, pkgId, { built }))
    }
    return [storeIndexKey(resolution.integrity, pkgId)]
  }
  switch (resolution.type) {
    case 'git':
      return [true, false].map((built) => gitHostedStoreIndexKey(pkgId, { built }))
    case 'binary':
      return [storeIndexKey(resolution.integrity, pkgId)]
    case 'variations':
      return resolution.variants.flatMap((variant) => packageIndexKeys(variant.resolution, pkgId))
    default:
      // A directory has no row, and the content of a custom one is not
      // known here.
      return []
  }
}

function * rowFiles (row: PackageFilesIndex): Iterable<PackageFileInfo> {
  yield * row.files.values()
  for (const sideEffectsDiff of row.sideEffects?.values() ?? []) {
    if (sideEffectsDiff.added != null) yield * sideEffectsDiff.added.values()
  }
}
