import { getTarballIntegrity } from '@pnpm/crypto.hash'
import { refToRelative } from '@pnpm/dependency-path'
import {
  type ProjectSnapshot,
  type PackageSnapshots,
  type TarballResolution,
} from '@pnpm/lockfile.types'
import { refIsLocalTarball } from '@pnpm/lockfile.utils'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import path from 'node:path'
import pEvery from 'p-every'

export interface LocalTarballDepsUpToDateContext {
  /**
   * Local cache of local absolute file paths to their integrity. Expected to be
   * initialized to an empty map by the caller.
   */
  readonly fileIntegrityCache: Map<string, Promise<string>>
  readonly lockfilePackages?: PackageSnapshots
  readonly lockfileDir: string
}

/**
 * Returns false if a local tarball file has been changed on disk since the last
 * installation recorded by the project snapshot.
 *
 * This function only inspects the project's lockfile snapshot. It does not
 * inspect the current project manifest. The caller of this function is expected
 * to handle changes to the project manifest that would cause the corresponding
 * project snapshot to become out of date.
 */
export async function localTarballDepsAreUpToDate (
  {
    fileIntegrityCache,
    lockfilePackages,
    lockfileDir,
  }: LocalTarballDepsUpToDateContext,
  project: {
    snapshot: ProjectSnapshot
  }
): Promise<boolean> {
  return pEvery(DEPENDENCIES_FIELDS, (depField) => {
    const lockfileDeps = project.snapshot[depField]

    // If the lockfile is missing a snapshot for this project's dependencies, we
    // can return true. The "satisfiesPackageManifest" logic in
    // "allProjectsAreUpToDate" will catch mismatches between a project's
    // manifest and snapshot dependencies size.
    if (lockfileDeps == null) {
      return true
    }

    return pEvery(
      Object.entries(lockfileDeps),
      async ([depName, ref]) => {
        if (!refIsLocalTarball(ref)) {
          return true
        }

        const depPath = refToRelative(ref, depName)
        const packageSnapshot = depPath != null ? lockfilePackages?.[depPath] : null

        // If there's no snapshot for this local tarball yet, the project is out
        // of date and needs to be resolved. This should only happen with a
        // broken lockfile.
        if (packageSnapshot == null) {
          return false
        }

        const filePath = path.join(lockfileDir, ref.slice('file:'.length))

        const fileIntegrityPromise = fileIntegrityCache.get(filePath) ?? getTarballIntegrity(filePath)
        if (!fileIntegrityCache.has(filePath)) {
          fileIntegrityCache.set(filePath, fileIntegrityPromise)
        }

        let fileIntegrity: string
        try {
          fileIntegrity = await fileIntegrityPromise
        } catch (err) {
          // If there was an error reading the tarball, assume the lockfile is
          // out of date. The full resolution process will emit a clearer error
          // later during install.
          return false
        }

        return (packageSnapshot.resolution as TarballResolution).integrity === fileIntegrity
      })
  })
}
