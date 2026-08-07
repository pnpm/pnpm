import * as dp from '@pnpm/deps.path'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { ProjectId, ProjectManifest } from '@pnpm/types'
import semver from 'semver'

interface Project {
  id: ProjectId
  manifest: ProjectManifest
}

export function hasChangedProjectSpecifiers (
  lockfile: LockfileObject,
  projects: Project[]
): boolean {
  return projects.some((project) => {
    const importer = lockfile.importers[project.id]
    if (importer == null) return false
    const manifestSpecifiers = getManifestSpecifiers(project.manifest)
    return Object.entries(manifestSpecifiers)
      .some(([alias, specifier]) => importer.specifiers[alias] !== specifier) ||
      // A dependency the importer records that the manifest dropped.
      Object.keys(importer.specifiers).some((alias) => manifestSpecifiers[alias] == null)
  })
}

export function tryFastUpdateImporters (
  lockfile: LockfileObject,
  projects: Project[]
): boolean {
  const updates: Array<{
    alias: string
    specifier: string
    specifiers: Record<string, string>
  }> = []
  for (const project of projects) {
    const importer = lockfile.importers[project.id]
    if (importer == null) return false
    for (const [alias, specifier] of Object.entries(getManifestSpecifiers(project.manifest))) {
      if (importer.specifiers[alias] === specifier) continue
      const reference =
        importer.optionalDependencies?.[alias] ??
        importer.dependencies?.[alias] ??
        importer.devDependencies?.[alias]
      const version = reference == null ? null : dp.removeSuffix(reference)
      if (
        semver.validRange(specifier) == null ||
        version == null ||
        semver.valid(version) == null ||
        !semver.satisfies(version, specifier)
      ) {
        return false
      }
      updates.push({
        alias,
        specifier,
        specifiers: importer.specifiers,
      })
    }
  }
  const dropped = new Set<string>()
  for (const project of projects) {
    const importer = lockfile.importers[project.id]
    const manifestSpecifiers = getManifestSpecifiers(project.manifest)
    for (const alias of Object.keys(importer.specifiers)) {
      if (manifestSpecifiers[alias] != null) continue
      dropped.add(alias)
      delete importer.specifiers[alias]
      delete importer.dependencies?.[alias]
      delete importer.devDependencies?.[alias]
      delete importer.optionalDependencies?.[alias]
    }
  }

  if (dropped.size > 0 && !peerSuffixesAreIndependentOf(lockfile, dropped)) return false

  for (const update of updates) {
    update.specifiers[update.alias] = update.specifier
  }
  if (dropped.size > 0) {
    const pruned = pruneSharedLockfile(lockfile)
    if (pruned.packages == null) {
      delete lockfile.packages
    } else {
      lockfile.packages = pruned.packages
    }
  }
  return updates.length > 0 || dropped.size > 0
}

/**
 * Whether no surviving package resolves a peer through one of `dropped`.
 *
 * A dropped package that some package reaches as a peer is embedded in that
 * package's key, so removing it would rekey the dependent rather than only
 * prune. A peer suffix pnpm shortened into a hash cannot be read to rule that
 * out.
 */
function peerSuffixesAreIndependentOf (lockfile: LockfileObject, dropped: Set<string>): boolean {
  return Object.keys(lockfile.packages ?? {}).every((depPath) => {
    const { peersIndex } = dp.indexOfDepPathSuffix(depPath)
    if (peersIndex === -1) return true
    const peers = depPath.substring(peersIndex)
    return peers
      .replace(/^\(/, '')
      .replace(/\)$/, '')
      .split(')(')
      .every((segment) => segment.includes('@')) &&
      ![...dropped].some((alias) => peers.includes(`${alias}@`))
  })
}

function getManifestSpecifiers (manifest: ProjectManifest): Record<string, string> {
  return {
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
}
