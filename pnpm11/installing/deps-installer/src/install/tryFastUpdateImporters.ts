import * as dp from '@pnpm/deps.path'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { ProjectId, ProjectManifest } from '@pnpm/types'
import { clone } from 'ramda'
import semver from 'semver'

import { pruneUnreferencedCatalogEntries } from './tryFastUpdateCatalogs.js'

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
      Object.keys(importer.specifiers).some((alias) => manifestSpecifiers[alias] == null)
  })
}

export function tryFastUpdateImporters (
  lockfile: LockfileObject,
  projects: Project[]
): boolean {
  const candidate = clone(lockfile)
  const dropped = new Set<string>()
  let changed = false
  for (const project of projects) {
    const importer = candidate.importers[project.id]
    if (importer == null) return false
    const manifestSpecifiers = getManifestSpecifiers(project.manifest)
    for (const [alias, specifier] of Object.entries(manifestSpecifiers)) {
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
      importer.specifiers[alias] = specifier
      changed = true
    }
    for (const alias of Object.keys(importer.specifiers)) {
      if (manifestSpecifiers[alias] != null) continue
      dropped.add(alias)
      delete importer.specifiers[alias]
      for (const group of ['dependencies', 'devDependencies', 'optionalDependencies'] as const) {
        delete importer[group]?.[alias]
        if (importer[group] != null && Object.keys(importer[group]).length === 0) {
          delete importer[group]
        }
      }
      changed = true
    }
  }
  if (!changed) return false

  if (dropped.size > 0) {
    const pruned = pruneSharedLockfile(candidate)
    if (pruned.packages == null) {
      delete candidate.packages
    } else {
      candidate.packages = pruned.packages
    }
    // Pruned first: a peer-dependent package that the removal itself makes
    // unreachable needs no rekeying, so only the survivors are checked.
    if (!peerSuffixesAreIndependentOf(candidate, dropped)) return false
    pruneUnreferencedCatalogEntries(candidate)
  }

  lockfile.importers = candidate.importers
  if (candidate.packages == null) {
    delete lockfile.packages
  } else {
    lockfile.packages = candidate.packages
  }
  if (candidate.catalogs == null) {
    delete lockfile.catalogs
  } else {
    lockfile.catalogs = candidate.catalogs
  }
  return true
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
