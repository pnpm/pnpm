import { getTarballIntegrity, matchIntegrity } from '@pnpm/crypto.hash'
import * as dp from '@pnpm/deps.path'
import { isError, PnpmError } from '@pnpm/error'
import type {
  PackageSnapshot,
  PackageSnapshots,
  ProjectSnapshot,
  TarballResolution,
} from '@pnpm/lockfile.types'
import { refIsLocalTarball, resolveLocalTarballPath } from '@pnpm/lockfile.utils'
import { DEPENDENCIES_FIELDS, type DependenciesField, type IncludedDependencies } from '@pnpm/types'

export interface LocalTarballDepsUpToDateContext {
  /**
   * Local cache of local absolute file paths to their integrity. Expected to be
   * initialized to an empty map by the caller.
   */
  readonly fileIntegrityCache: Map<string, Promise<string>>
  /**
   * The dependency paths a call already checked, so that each is checked
   * once. Share one only across calls whose results the caller requires to
   * all be `true`.
   */
  readonly walkedDepPaths?: Set<string>
  readonly includedDependencies?: IncludedDependencies
  readonly lockfilePackages?: PackageSnapshots
  readonly lockfileDir: string
}

export interface LocalTarballIntegrityMismatch {
  readonly expected: string
  readonly found: string
  readonly path: string
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
    .filter((field) => isIncluded(ctx.includedDependencies, field))
    .flatMap((field) => Object.entries(project.snapshot[field] ?? {}))
  const reached = reachableDependencies(ctx, dependencies)
  const results = await Promise.all(reached.map(async ([depName, ref]) => localTarballDepIsUpToDate(ctx, depName, ref)))
  return results.every(Boolean)
}

/**
 * Whether an install with `included` installs a project's `field`. Optional
 * dependencies go with the production ones.
 */
function isIncluded (included: IncludedDependencies | undefined, field: DependenciesField): boolean {
  if (included?.[field] === false) return false
  return field !== 'optionalDependencies' || included?.dependencies !== false
}

/**
 * `dependencies` and every dependency they reach through the lockfile
 * packages, without the optional ones the install excludes. A project
 * reaches a local tarball through a workspace project it injects, whose
 * dependencies the project's own lockfile records.
 */
function reachableDependencies (
  { lockfilePackages: packages, includedDependencies, walkedDepPaths }: Pick<LocalTarballDepsUpToDateContext, 'lockfilePackages' | 'includedDependencies' | 'walkedDepPaths'>,
  dependencies: Array<[string, string]>
): Array<[string, string]> {
  const includesOptional = includedDependencies?.optionalDependencies !== false
  const reached: Array<[string, string]> = []
  const visited = walkedDepPaths ?? new Set<string>()
  const queue = [...dependencies]
  for (let next = queue.pop(); next != null; next = queue.pop()) {
    const [depName, ref] = next
    if (typeof ref !== 'string') continue
    const depPath = dp.refToRelative(ref, depName)
    if (depPath == null || visited.has(depPath)) continue
    visited.add(depPath)
    reached.push(next)
    const snapshot = packages?.[depPath]
    queue.push(...Object.entries(snapshot?.dependencies ?? {}))
    if (includesOptional) queue.push(...Object.entries(snapshot?.optionalDependencies ?? {}))
  }
  return reached
}

async function localTarballDepIsUpToDate (
  ctx: LocalTarballDepsUpToDateContext,
  depName: string,
  ref: string
): Promise<boolean> {
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
