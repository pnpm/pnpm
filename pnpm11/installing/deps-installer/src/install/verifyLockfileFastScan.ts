import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import { isNonLocalFilesystemSubdepSpec } from '@pnpm/resolving.local-resolver'
import type { DepPath } from '@pnpm/types'

import {
  hasLocalFilesystemResolution,
  hasLocalGitResolution,
  isLocalGitKey,
} from './verifyLockfileShape.js'

/**
 * Single linear pre-scan over `lockfile` returning `false` when no importer or
 * snapshot contains any local (`link:`, `file:`, `directory`, `file:` tarball,
 * `git+file:` / local `git`, or local `peerDependencies`) reference, skipping
 * per-importer BFS on pure registry lockfiles.
 */
export function lockfileHasAnyLocalSurface (lockfile: LockfileObject): boolean {
  return importersHaveLocalSurface(lockfile) || packagesHaveLocalSurface(lockfile)
}

function importersHaveLocalSurface (lockfile: LockfileObject): boolean {
  for (const importer of Object.values(lockfile.importers ?? {})) {
    for (const depMap of [importer.dependencies, importer.devDependencies, importer.optionalDependencies]) {
      if (depMapHasLocalRef(depMap, true)) return true
    }
  }
  return false
}

function packagesHaveLocalSurface (lockfile: LockfileObject): boolean {
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    if (snapshotEntryHasLocalSurface(depPath, snapshot)) return true
  }
  return false
}

function snapshotEntryHasLocalSurface (depPath: DepPath, snapshot: PackageSnapshot): boolean {
  const parsed = dp.parse(depPath)
  if (parsed.nonSemverVersion?.startsWith('file:') || isLocalGitKey(parsed.nonSemverVersion)) return true
  if (hasLocalFilesystemResolution(snapshot.resolution) || hasLocalGitResolution(snapshot.resolution)) return true
  if (depMapHasLocalRef(snapshot.dependencies, false) || depMapHasLocalRef(snapshot.optionalDependencies, false)) {
    return true
  }
  return peersHaveLocalSurface(snapshot.peerDependencies)
}

function depMapHasLocalRef (depMap: Record<string, string> | undefined, allowInternalLink: boolean): boolean {
  if (depMap == null) return false
  for (const [alias, ref] of Object.entries(depMap) as Array<[string, unknown]>) {
    if (typeof ref !== 'string') {
      throw new PnpmError('BROKEN_LOCKFILE', `The lockfile is broken: the dependency "${alias}" has a non-string reference`)
    }
    if (depRefIsLocal(alias, ref, allowInternalLink)) return true
  }
  return false
}

function depRefIsLocal (alias: string, ref: string, allowInternalLink: boolean): boolean {
  if (ref.startsWith('link:')) {
    return allowInternalLink || dp.packageRootLinkTarget(ref) == null
  }
  const childDepPath = dp.refToRelative(ref, alias)
  if (childDepPath == null) return false
  const nonSemver = dp.parse(childDepPath).nonSemverVersion
  return Boolean(nonSemver?.startsWith('file:')) || isLocalGitKey(nonSemver)
}

function peersHaveLocalSurface (peers: Record<string, string> | undefined): boolean {
  if (peers == null) return false
  for (const spec of Object.values(peers)) {
    if (dp.packageRootLinkTarget(spec) == null && isNonLocalFilesystemSubdepSpec(spec, () => false)) {
      return true
    }
  }
  return false
}
