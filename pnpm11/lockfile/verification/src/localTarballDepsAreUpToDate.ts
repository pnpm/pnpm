import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { getTarballIntegrity, matchIntegrity } from '@pnpm/crypto.hash'
import * as dp from '@pnpm/deps.path'
import { isError, PnpmError } from '@pnpm/error'
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
    return localFileUrlToPath(tarball)
  }
  if (path.isAbsolute(pathPart)) {
    return path.normalize(pathPart)
  }
  return path.resolve(lockfileDir, pathPart)
}

function localFileUrlToPath (tarball: string): string | undefined {
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

export async function findPackageTarballIntegrityMismatch (
  ctx: Pick<LocalTarballDepsUpToDateContext, 'fileIntegrityCache' | 'lockfileDir'>,
  snapshot?: PackageSnapshot,
  depPath?: string
): Promise<LocalTarballIntegrityMismatch | null> {
  const resolution = snapshot?.resolution as TarballResolution | undefined
  if (resolution == null || typeof resolution !== 'object') return null
  const tarball = getResolutionTarball(resolution, depPath)
  if (typeof tarball !== 'string' || typeof resolution.integrity !== 'string' || !resolution.integrity.trim()) return null
  const filePath = resolveLocalTarballPath(ctx.lockfileDir, tarball)
  if (filePath == null) return null
  const found = await readLocalTarballIntegrity(ctx.fileIntegrityCache, filePath)
  const result = matchIntegrity(found, resolution.integrity)
  return result.matches ? null : { expected: resolution.integrity, found: result.found, path: filePath }
}

/**
 * The resolution's tarball, or the one the dependency path names. `undefined`
 * when neither has one or the dependency path cannot be parsed.
 */
function getResolutionTarball (resolution: TarballResolution, depPath: string | undefined): string | undefined {
  if (typeof resolution.tarball === 'string') return resolution.tarball
  if (typeof depPath !== 'string') return undefined
  try {
    return dp.parse(depPath).nonSemverVersion
  } catch {
    return undefined
  }
}

function readLocalTarballIntegrity (fileIntegrityCache: Map<string, Promise<string>>, filePath: string): Promise<string> {
  let integrity = fileIntegrityCache.get(filePath)
  if (integrity == null) {
    integrity = getTarballIntegrity(filePath, {
      algorithms: ['sha512', 'sha384', 'sha256', 'sha1'],
    }).catch((error: unknown) => {
      fileIntegrityCache.delete(filePath)
      const message = isError(error) ? error.message : String(error)
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
  ctx: LocalTarballDepsUpToDateContext,
  project: {
    snapshot: ProjectSnapshot
  }
): Promise<boolean> {
  const dependencies = DEPENDENCIES_FIELDS
    .filter((field) => ctx.includedDependencies?.[field] !== false)
    .flatMap((field) => Object.entries(project.snapshot[field] ?? {}))
  const results = await Promise.all(dependencies.map(async ([depName, ref]) => localTarballDepIsUpToDate(ctx, depName, ref)))
  return results.every(Boolean)
}

async function localTarballDepIsUpToDate (
  ctx: LocalTarballDepsUpToDateContext,
  depName: string,
  ref: string
): Promise<boolean> {
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

  const packageSnapshot = ctx.lockfilePackages?.[depPath]

  // If there's no snapshot for this local tarball yet, the project is out
  // of date and needs to be resolved. This should only happen with a
  // broken lockfile.
  if (packageSnapshot == null) {
    return false
  }

  return localTarballMatchesSnapshot(ctx, tarballRefWithoutPeersSuffix, packageSnapshot)
}

async function localTarballMatchesSnapshot (
  ctx: LocalTarballDepsUpToDateContext,
  tarballRef: string,
  packageSnapshot: PackageSnapshot
): Promise<boolean> {
  const filePath = resolveLocalTarballPath(ctx.lockfileDir, tarballRef)
  if (filePath == null) {
    return false
  }

  let fileIntegrity: string
  try {
    fileIntegrity = await readLocalTarballIntegrity(ctx.fileIntegrityCache, filePath)
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
}
