import path from 'node:path'
import { fileURLToPath } from 'node:url'
import util from 'node:util'

import { getTarballIntegrity, matchIntegrity } from '@pnpm/crypto.hash'
import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type {
  PackageSnapshot,
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

function isUncLikeFilePayload (pathPart: string): boolean {
  return pathPart.startsWith('\\\\') ||
    pathPart.startsWith('////') ||
    (pathPart.startsWith('//') && !pathPart.startsWith('///'))
}

export function resolveLocalTarballPath (lockfileDir: string, tarball: string): string | undefined {
  if (!refIsLocalTarball(tarball)) return undefined
  const pathPart = tarball.slice('file:'.length)
  if (isUncLikeFilePayload(pathPart) || pathPart.includes('\0')) {
    return undefined
  }
  if (pathPart.startsWith('/') && tarball.startsWith('file:///')) {
    try {
      const url = new URL(tarball)
      if (url.protocol !== 'file:' || url.host) {
        return undefined
      }
      return fileURLToPath(url)
    } catch {
      return undefined
    }
  }
  if (path.isAbsolute(pathPart)) {
    return path.normalize(pathPart)
  }
  return path.resolve(lockfileDir, pathPart)
}

export async function findPackageTarballIntegrityMismatch (
  ctx: Pick<LocalTarballDepsUpToDateContext, 'fileIntegrityCache' | 'lockfileDir'>,
  snapshot?: PackageSnapshot,
  depPath?: string
): Promise<LocalTarballIntegrityMismatch | null> {
  const resolution = snapshot?.resolution as TarballResolution | undefined
  if (resolution == null || typeof resolution !== 'object') return null
  let tarball = typeof resolution.tarball === 'string' ? resolution.tarball : undefined
  if (tarball == null && typeof depPath === 'string') {
    try {
      tarball = dp.parse(depPath).nonSemverVersion
    } catch {
      return null
    }
  }
  if (typeof tarball !== 'string' || typeof resolution.integrity !== 'string' || !resolution.integrity.trim()) return null
  const filePath = resolveLocalTarballPath(ctx.lockfileDir, tarball)
  if (filePath == null) return null
  const found = await readLocalTarballIntegrity(ctx.fileIntegrityCache, filePath)
  const result = matchIntegrity(found, resolution.integrity)
  return result.matches ? null : { expected: resolution.integrity, found: result.found, path: filePath }
}

function readLocalTarballIntegrity (fileIntegrityCache: Map<string, Promise<string>>, filePath: string): Promise<string> {
  let integrity = fileIntegrityCache.get(filePath)
  if (integrity == null) {
    integrity = getTarballIntegrity(filePath, {
      algorithms: ['sha512', 'sha384', 'sha256', 'sha1'],
    }).catch((error: unknown) => {
      fileIntegrityCache.delete(filePath)
      const message = util.types.isNativeError(error) ? error.message : String(error)
      throw new PnpmError(
        'TARBALL_READ_LOCAL_TARBALL',
        `Cannot read local tarball "${filePath}": ${message}`,
        { cause: error }
      )
    })
    fileIntegrityCache.set(filePath, integrity)
  }
  return integrity
}

export async function localTarballDepsAreUpToDate (
  {
    fileIntegrityCache,
    includedDependencies,
    lockfilePackages,
    lockfileDir,
  }: LocalTarballDepsUpToDateContext,
  project: {
    snapshot: ProjectSnapshot
  }
): Promise<boolean> {
  const dependencies = DEPENDENCIES_FIELDS
    .filter((field) => includedDependencies?.[field] !== false)
    .flatMap((field) => Object.entries(project.snapshot[field] ?? {}))
  const results = await Promise.all(dependencies.map(async ([depName, ref]) => {
    if (!ref.startsWith('file:')) {
      return true
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
      return true
    }
    const parsed = dp.parse(depPath)
    const tarballRefWithoutPeersSuffix = parsed.nonSemverVersion

    // Tarball refs aren't "semver" versions. If the nonSemverVersion field
    // is empty, this isn't a depPath for a tarball.
    if (tarballRefWithoutPeersSuffix == null) {
      return true
    }

    if (!refIsLocalTarball(tarballRefWithoutPeersSuffix)) {
      return true
    }

    const packageSnapshot = lockfilePackages?.[depPath]

    // If there's no snapshot for this local tarball yet, the project is out
    // of date and needs to be resolved. This should only happen with a
    // broken lockfile.
    if (packageSnapshot == null) {
      return false
    }

    const filePath = resolveLocalTarballPath(lockfileDir, tarballRefWithoutPeersSuffix)
    if (filePath == null) {
      return false
    }

    let fileIntegrity: string
    try {
      fileIntegrity = await readLocalTarballIntegrity(fileIntegrityCache, filePath)
    } catch (_err) {
      // If there was an error reading the tarball, assume the lockfile is
      // out of date. The full resolution process will emit a clearer error
      // later during install.
      return false
    }

    const packageSnapshotResolution = packageSnapshot.resolution as TarballResolution | undefined
    const expected = packageSnapshotResolution?.integrity
    if (typeof expected !== 'string' || !expected.trim()) {
      return false
    }
    return matchIntegrity(fileIntegrity, expected).matches
  }))
  return results.every(Boolean)
}
