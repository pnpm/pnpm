import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath, ProjectId, ProjectManifest } from '@pnpm/types'
import semver from 'semver'

/**
 * Walk the resolved lockfile to determine which override selectors matched
 * at least one dependency. Used on the pnpr-server path where the resolver
 * runs server-side and does not report applied selectors back.
 *
 * Two indexes are precomputed once before the override loop:
 * - `packagesByName`: maps each package name to its snapshots + versions,
 *   so parent-scoped lookups skip every package that doesn't match the
 *   parent name (O(1) per override instead of O(n)).
 * - `allDepNames`: union of every dependency key across importers,
 *   snapshots, and project manifests' peerDependencies, so non-parent
 *   overrides are a single Set membership check.
 */
export function findAppliedOverrideSelectorsFromLockfile (
  lockfile: LockfileObject,
  parsedOverrides: Array<{ selector: string, newBareSpecifier?: string, parentPkg?: { name: string, bareSpecifier?: string }, targetPkg: { name: string, bareSpecifier?: string } }>,
  projectManifests: Array<{ importerId: string, manifest: ProjectManifest }> = []
): Set<string> {
  const applied = new Set<string>()

  // Precompute packagesByName: parent name → snapshots + versions.
  const packagesByName = new Map<string, Array<{ snapshot: PackageSnapshot, version: string | undefined }>>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    const list = packagesByName.get(name)
    if (list != null) {
      list.push({ snapshot, version })
    } else {
      packagesByName.set(name, [{ snapshot, version }])
    }
  }

  // Precompute allDepNames: every dependency key across all sources.
  const allDepNames = new Set<string>()
  for (const importer of Object.values(lockfile.importers)) {
    addKeys(allDepNames, importer.dependencies)
    addKeys(allDepNames, importer.devDependencies)
    addKeys(allDepNames, importer.optionalDependencies)
  }
  for (const entries of packagesByName.values()) {
    for (const { snapshot } of entries) {
      addKeys(allDepNames, snapshot.dependencies)
      addKeys(allDepNames, snapshot.optionalDependencies)
      addKeys(allDepNames, snapshot.peerDependencies)
    }
  }
  for (const { manifest } of projectManifests) {
    addKeys(allDepNames, manifest.peerDependencies)
  }

  for (const override of parsedOverrides) {
    // Delete overrides remove the dependency key before resolution,
    // so the lockfile never contains it. Treat as applied.
    if (override.newBareSpecifier === '-') {
      applied.add(override.selector)
      continue
    }

    const targetName = override.targetPkg.name

    if (override.parentPkg != null) {
      const parentName = override.parentPkg.name
      const parentRange = override.parentPkg.bareSpecifier
      const parentRangeValid = parentRange == null || semver.validRange(parentRange) != null

      // Check workspace project manifests as potential parent matches.
      for (const { importerId, manifest: projectManifest } of projectManifests) {
        if (projectManifest.name !== parentName) continue
        if (parentRange != null) {
          const projectVersion = projectManifest.version
          if (projectVersion == null) continue
          if (!parentRangeValid || !semver.satisfies(projectVersion, parentRange)) continue
        }
        const importer = lockfile.importers[importerId as ProjectId]
        if (
          (importer != null && targetInImporter(importer, targetName)) ||
          depEntryMatches(projectManifest.peerDependencies, targetName)
        ) {
          applied.add(override.selector)
          break
        }
      }
      if (applied.has(override.selector)) continue

      // Check resolved packages matching the parent name.
      const parentEntries = packagesByName.get(parentName)
      if (parentEntries != null) {
        for (const { snapshot, version } of parentEntries) {
          if (parentRange != null && (version == null || !parentRangeValid || !semver.satisfies(version, parentRange))) continue
          if (targetInSnapshot(snapshot, targetName)) {
            applied.add(override.selector)
            break
          }
        }
      }
    } else {
      if (allDepNames.has(targetName)) {
        applied.add(override.selector)
      }
    }
  }

  return applied
}

function addKeys (set: Set<string>, deps: Record<string, string> | undefined): void {
  if (deps == null) return
  for (const key of Object.keys(deps)) set.add(key)
}

/**
 * Check whether an importer's dependency groups contain `targetName`.
 */
function targetInImporter (importer: { dependencies?: Record<string, string>, devDependencies?: Record<string, string>, optionalDependencies?: Record<string, string> }, targetName: string): boolean {
  return depEntryMatches(importer.dependencies, targetName) ||
    depEntryMatches(importer.devDependencies, targetName) ||
    depEntryMatches(importer.optionalDependencies, targetName)
}

/**
 * Check whether a package snapshot's dependency groups contain `targetName`.
 */
function targetInSnapshot (snapshot: PackageSnapshot, targetName: string): boolean {
  return depEntryMatches(snapshot.dependencies, targetName) ||
    depEntryMatches(snapshot.optionalDependencies, targetName) ||
    depEntryMatches(snapshot.peerDependencies, targetName)
}

function depEntryMatches (
  deps: Record<string, string> | undefined,
  targetName: string
): boolean {
  if (deps == null) return false
  return deps[targetName] != null
}
