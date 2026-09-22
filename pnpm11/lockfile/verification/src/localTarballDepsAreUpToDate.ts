import path from 'node:path'

import { getTarballIntegrity } from '@pnpm/crypto.hash'
import * as dp from '@pnpm/deps.path'
import type {
  PackageSnapshots,
  ProjectSnapshot,
  TarballResolution,
} from '@pnpm/lockfile.types'
import { refIsLocalTarball } from '@pnpm/lockfile.utils'
import { DEPENDENCIES_FIELDS, type IncludedDependencies } from '@pnpm/types'

export interface LocalTarballDepsUpToDateContext {
  /**
   * Local cache of local absolute file paths to their integrity. Expected to be
   * initialized to an empty map by the caller.
   */
  readonly fileIntegrityCache: Map<string, Promise<string>>
  readonly includedDependencies?: IncludedDependencies
  readonly lockfilePackages?: PackageSnapshots
  readonly lockfileDir: string
}

export interface LocalTarballIntegrityMismatch {
  readonly expected: string
  readonly found: string
  readonly path: string
}

type LocalTarballDepsCheckResult =
  | { readonly kind: 'up-to-date' }
  | { readonly kind: 'unavailable' }
  | { readonly kind: 'mismatch', readonly mismatch: LocalTarballIntegrityMismatch }

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
  ctx: LocalTarballDepsUpToDateContext,
  project: {
    snapshot: ProjectSnapshot
  }
): Promise<boolean> {
  return (await checkLocalTarballDeps(ctx, project)).kind === 'up-to-date'
}

export async function findLocalTarballIntegrityMismatch (
  ctx: LocalTarballDepsUpToDateContext,
  project: {
    snapshot: ProjectSnapshot
  }
): Promise<LocalTarballIntegrityMismatch | null> {
  const result = await checkLocalTarballDeps(ctx, project)
  return result.kind === 'mismatch' ? result.mismatch : null
}

async function checkLocalTarballDeps (
  {
    fileIntegrityCache,
    includedDependencies,
    lockfilePackages,
    lockfileDir,
  }: LocalTarballDepsUpToDateContext,
  project: {
    snapshot: ProjectSnapshot
  }
): Promise<LocalTarballDepsCheckResult> {
  for (const depField of DEPENDENCIES_FIELDS) {
    if (includedDependencies?.[depField] === false) continue
    const lockfileDeps = project.snapshot[depField]

    // If the lockfile is missing a snapshot for this project's dependencies, we
    // can return true. The "satisfiesPackageManifest" logic in
    // "allProjectsAreUpToDate" will catch mismatches between a project's
    // manifest and snapshot dependencies size.
    if (lockfileDeps == null) {
      continue
    }

    const results = await Promise.all(Object.entries(lockfileDeps).map(async ([depName, ref]) => {
      if (!ref.startsWith('file:')) {
        return { kind: 'up-to-date' } as const
      }

      // The tarball ref can contain peers. Ex: file:bar.tgz(react@19.1.0)
      //
      // Trim out the peer suffix version to get a path to the local tarball.
      //
      //   - file:bar.tgz               → file:bar.tgz
      //   - file:bar.tgz(react@19.1.0) → file:bar.tgz
      //
      const depPath = dp.refToRelative(ref, depName)
      if (depPath == null) {
        return { kind: 'up-to-date' } as const
      }
      const parsed = dp.parse(depPath)
      const tarballRefWithoutPeersSuffix = parsed.nonSemverVersion

      // Tarball refs aren't "semver" versions. If the nonSemverVersion field
      // is empty, this isn't a depPath for a tarball.
      if (tarballRefWithoutPeersSuffix == null) {
        return { kind: 'up-to-date' } as const
      }

      if (!refIsLocalTarball(tarballRefWithoutPeersSuffix)) {
        return { kind: 'up-to-date' } as const
      }

      const packageSnapshot = lockfilePackages?.[depPath]

      // If there's no snapshot for this local tarball yet, the project is out
      // of date and needs to be resolved. This should only happen with a
      // broken lockfile.
      if (packageSnapshot == null) {
        return { kind: 'unavailable' } as const
      }

      const fileRelativePath = tarballRefWithoutPeersSuffix.slice('file:'.length)
      const filePath = path.join(lockfileDir, fileRelativePath)

      const fileIntegrityPromise = fileIntegrityCache.get(filePath) ?? getTarballIntegrity(filePath)
      if (!fileIntegrityCache.has(filePath)) {
        fileIntegrityCache.set(filePath, fileIntegrityPromise)
      }

      let fileIntegrity: string
      try {
        fileIntegrity = await fileIntegrityPromise
      } catch (_err) {
        // If there was an error reading the tarball, assume the lockfile is
        // out of date. The full resolution process will emit a clearer error
        // later during install.
        return { kind: 'unavailable' } as const
      }

      const expected = (packageSnapshot.resolution as TarballResolution).integrity
      if (typeof expected !== 'string') {
        return { kind: 'unavailable' } as const
      }
      if (expected === fileIntegrity) {
        return { kind: 'up-to-date' } as const
      }
      return {
        kind: 'mismatch',
        mismatch: { expected, found: fileIntegrity, path: filePath },
      } as const
    }))
    const result = results.find((result) => result.kind !== 'up-to-date')
    if (result != null) return result
  }
  return { kind: 'up-to-date' }
}
