import * as dp from '@pnpm/deps.path'
import { isValidDependencyAlias } from '@pnpm/installing.deps-resolver'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.fs'
import { isGitHostedTarballUrl, nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { Resolution, ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'

export const RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE = 'RESOLUTION_SHAPE_MISMATCH'

export function hasOfflineLockfileViolations (lockfile: LockfileObject): boolean {
  return collectInvalidAliases(lockfile).length > 0 || collectShapeViolations(lockfile).length > 0
}

export function collectInvalidAliases (lockfile: LockfileObject): string[] {
  const invalidAliases = new Set<string>()
  for (const importer of Object.values(lockfile.importers ?? {})) {
    pushInvalidAliases(importer.dependencies, invalidAliases)
    pushInvalidAliases(importer.devDependencies, invalidAliases)
    pushInvalidAliases(importer.optionalDependencies, invalidAliases)
  }
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name } = nameVerFromPkgSnapshot(depPath as DepPath, snapshot)
    if (!name || !isValidDependencyAlias(name)) invalidAliases.add(name || depPath)
    pushInvalidAliases(snapshot.dependencies, invalidAliases)
    pushInvalidAliases(snapshot.optionalDependencies, invalidAliases)
  }
  return Array.from(invalidAliases)
}

export function collectShapeViolations (lockfile: LockfileObject): ResolutionPolicyViolation[] {
  const shapeViolations: ResolutionPolicyViolation[] = []
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath as DepPath, snapshot)
    if (!name || !version) continue
    if (nonSemverVersion == null && !isRegistryShapedResolution(snapshot.resolution)) {
      shapeViolations.push(createShapeViolation(name, version, snapshot.resolution))
    }
  }
  return shapeViolations
}

export function collectLockfileFileOfflineViolations (lockfile: LockfileFile): {
  invalidAliases: string[]
  shapeViolations: ResolutionPolicyViolation[]
} {
  const invalidAliases = new Set<string>()
  const shapeViolations: ResolutionPolicyViolation[] = []
  for (const importer of Object.values(lockfile.importers ?? {})) {
    pushInvalidAliases(importer.dependencies, invalidAliases)
    pushInvalidAliases(importer.devDependencies, invalidAliases)
    pushInvalidAliases(importer.optionalDependencies, invalidAliases)
  }
  collectLockfileFilePackageViolations(lockfile.packages, invalidAliases, shapeViolations)
  collectLockfileFileSnapshotAliases(lockfile.snapshots, invalidAliases)
  return { invalidAliases: Array.from(invalidAliases), shapeViolations }
}

function collectLockfileFilePackageViolations (
  packages: LockfileFile['packages'],
  invalidAliases: Set<string>,
  shapeViolations: ResolutionPolicyViolation[]
): void {
  for (const [depPath, pkgInfo] of Object.entries(packages ?? {})) {
    const parsed = dp.parse(depPath)
    if (!parsed.name || !isValidDependencyAlias(parsed.name)) {
      invalidAliases.add(parsed.name ?? depPath)
    }
    const version = pkgInfo.version ?? parsed.version
    if (!parsed.name || !version) continue
    if (parsed.nonSemverVersion == null && !isRegistryShapedResolution(pkgInfo.resolution)) {
      shapeViolations.push(createShapeViolation(parsed.name, version, pkgInfo.resolution))
    }
  }
}

function collectLockfileFileSnapshotAliases (
  snapshots: LockfileFile['snapshots'],
  invalidAliases: Set<string>
): void {
  for (const [depPath, snapshot] of Object.entries(snapshots ?? {})) {
    const parsed = dp.parse(depPath)
    if (!parsed.name || !isValidDependencyAlias(parsed.name)) {
      invalidAliases.add(parsed.name ?? depPath)
    }
    pushInvalidAliases(snapshot.dependencies, invalidAliases)
    pushInvalidAliases(snapshot.optionalDependencies, invalidAliases)
  }
}

function createShapeViolation (name: string, version: string, resolution: unknown): ResolutionPolicyViolation {
  return {
    name,
    version,
    resolution: resolution as Resolution,
    code: RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
    reason: 'a registry-style dependency path is backed by a non-registry resolution',
  }
}

function pushInvalidAliases (deps: Record<string, unknown> | undefined, invalid: Set<string>): void {
  if (deps == null) return
  for (const alias of Object.keys(deps)) {
    if (!isValidDependencyAlias(alias)) invalid.add(alias)
  }
}

function isRegistryShapedResolution (resolution: unknown): boolean {
  if (resolution == null) return true
  if (typeof resolution !== 'object') return false
  const { type, gitHosted, tarball, variants } = resolution as {
    type?: unknown
    gitHosted?: unknown
    tarball?: unknown
    variants?: unknown
  }
  if (type === 'variations') return areRegistryShapedVariants(variants)
  if (typeof type === 'string' && type.startsWith('custom:')) return true
  if (type != null) return false
  return isRegistryShapedTarballResolution(gitHosted, tarball)
}

function areRegistryShapedVariants (variants: unknown): boolean {
  return Array.isArray(variants) && variants.length > 0 && variants.every((variant) => {
    const resolution = (variant as { resolution?: unknown })?.resolution
    return resolution != null && isRegistryShapedResolution(resolution)
  })
}

function isRegistryShapedTarballResolution (gitHosted: unknown, tarball: unknown): boolean {
  if (gitHosted != null && (typeof gitHosted !== 'boolean' || gitHosted)) return false
  if (typeof tarball !== 'string' || tarball === '') return true
  return /^https?:\/\//i.test(tarball) && !isGitHostedTarballUrl(tarball)
}
