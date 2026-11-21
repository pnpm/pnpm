import * as dp from '@pnpm/deps.path'
import { isValidDependencyAlias } from '@pnpm/installing.deps-resolver'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.fs'
import type { Resolution, ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'

import { type FileShapeContext, shapeMismatchReason } from './verifyLockfileShape.js'

export const RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE = 'RESOLUTION_SHAPE_MISMATCH'

export function hasInvalidDependencyAliases (lockfile: LockfileObject): boolean {
  return collectInvalidAliases(lockfile).length > 0
}

export function collectInvalidAliases (lockfile: LockfileObject): string[] {
  const invalidAliases = new Set<string>()
  for (const importer of Object.values(lockfile.importers ?? {})) {
    pushInvalidAliases(importer.dependencies, invalidAliases)
    pushInvalidAliases(importer.devDependencies, invalidAliases)
    pushInvalidAliases(importer.optionalDependencies, invalidAliases)
  }
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const parsed = parseLockfileDepPath(depPath)
    if (parsed.name != null && !isValidDependencyAlias(parsed.name)) {
      invalidAliases.add(parsed.name)
    }
    pushInvalidAliases(snapshot.dependencies, invalidAliases)
    pushInvalidAliases(snapshot.optionalDependencies, invalidAliases)
  }
  return Array.from(invalidAliases)
}

export function collectShapeViolations (
  lockfile: LockfileObject,
  shapeCtx?: FileShapeContext
): ResolutionPolicyViolation[] {
  const shapeViolations: ResolutionPolicyViolation[] = []
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const parsed = parseLockfileDepPath(depPath)
    const displayVersion = snapshot.version ?? parsed.version ?? parsed.nonSemverVersion
    if (!parsed.name || !displayVersion) continue
    const reason = shapeMismatchReason(snapshot.resolution, parsed.nonSemverVersion, shapeCtx)
    if (reason != null) {
      shapeViolations.push(createShapeViolation(parsed.name, displayVersion, snapshot.resolution, reason))
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
    const parsed = parseLockfileDepPath(depPath)
    if (parsed.name != null && !isValidDependencyAlias(parsed.name)) {
      invalidAliases.add(parsed.name)
    }
    const displayVersion = pkgInfo.version ?? parsed.version ?? parsed.nonSemverVersion
    if (!parsed.name || !displayVersion) continue
    const reason = shapeMismatchReason(pkgInfo.resolution, parsed.nonSemverVersion)
    if (reason != null) {
      shapeViolations.push(createShapeViolation(parsed.name, displayVersion, pkgInfo.resolution, reason))
    }
  }
}

function collectLockfileFileSnapshotAliases (
  snapshots: LockfileFile['snapshots'],
  invalidAliases: Set<string>
): void {
  for (const [depPath, snapshot] of Object.entries(snapshots ?? {})) {
    const parsed = parseLockfileDepPath(depPath)
    if (parsed.name != null && !isValidDependencyAlias(parsed.name)) {
      invalidAliases.add(parsed.name)
    }
    pushInvalidAliases(snapshot.dependencies, invalidAliases)
    pushInvalidAliases(snapshot.optionalDependencies, invalidAliases)
  }
}

function parseLockfileDepPath (depPath: string): dp.DependencyPath {
  return dp.parse(depPath.startsWith('/') ? depPath.slice(1) : depPath)
}

function createShapeViolation (
  name: string,
  version: string,
  resolution: unknown,
  reason: string
): ResolutionPolicyViolation {
  return {
    name,
    version,
    resolution: resolution as Resolution,
    code: RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
    reason,
  }
}

function pushInvalidAliases (deps: Record<string, unknown> | undefined, invalid: Set<string>): void {
  if (deps == null) return
  for (const alias of Object.keys(deps)) {
    if (!isValidDependencyAlias(alias)) invalid.add(alias)
  }
}
