import * as dp from '@pnpm/deps.path'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { DEPENDENCIES_FIELDS, type DependenciesField, type ProjectId, type ProjectManifest } from '@pnpm/types'
import semver from 'semver'

import type { GraphEdits } from './tryComposeFastUpdates.js'

export interface Project {
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
      .some(([alias, specifier]) =>
        importer.specifiers[alias] !== specifier ||
        dependencyGroupMoved(importer, project.manifest, alias)) ||
      Object.keys(importer.specifiers).some((alias) => manifestSpecifiers[alias] == null)
  })
}

/**
 * Mutates `lockfile` in place and may leave it partially rewritten when it
 * returns `false` — the caller passes the coordinator's disposable candidate,
 * which is discarded on failure. The dropped aliases and optionality moves
 * are recorded in `edits` for the shared epilogue.
 */
export function tryFastUpdateImporters (
  lockfile: LockfileObject,
  projects: Project[],
  edits: GraphEdits
): boolean {
  let changed = false
  for (const project of projects) {
    const importer = lockfile.importers[project.id]
    if (importer == null) return false
    let editedGroups = false
    const manifestSpecifiers = getManifestSpecifiers(project.manifest)
    for (const [alias, specifier] of Object.entries(manifestSpecifiers)) {
      if (importer.specifiers[alias] !== specifier) {
        const recordedIn = recordedDependencyGroup(importer, alias)
        const reference = recordedIn == null ? undefined : importer[recordedIn]![alias]
        const version = reference == null ? null : dp.removeSuffix(reference)
        if (semver.validRange(specifier) == null || version == null || semver.valid(version) == null) {
          return false
        }
        const wanted = highestLockedVersionSatisfying(lockfile, alias, specifier)
        if (wanted == null) return false
        if (wanted !== version) {
          // The alias moves to a version the lockfile already holds, so its
          // subtree is already recorded. The old one may now be unreachable,
          // which the shared epilogue prunes.
          if (reference !== version) return false
          importer[recordedIn!]![alias] = wanted
          edits.dropped.add(alias)
        }
        importer.specifiers[alias] = specifier
        changed = true
      }
      const recordedIn = recordedDependencyGroup(importer, alias)
      const targetGroup = effectiveDependencyGroup(project.manifest, alias)
      if (recordedIn == null || recordedIn === targetGroup) continue
      const target = importer[targetGroup] ??= {}
      target[alias] = importer[recordedIn]![alias]
      delete importer[recordedIn]![alias]
      if (recordedIn === 'optionalDependencies' || targetGroup === 'optionalDependencies') {
        edits.movedAcrossOptional = true
      }
      editedGroups = true
      changed = true
    }
    for (const alias of Object.keys(importer.specifiers)) {
      if (manifestSpecifiers[alias] != null) continue
      edits.dropped.add(alias)
      delete importer.specifiers[alias]
      for (const group of DEPENDENCIES_FIELDS) {
        delete importer[group]?.[alias]
      }
      editedGroups = true
      changed = true
    }
    if (editedGroups) {
      for (const group of DEPENDENCIES_FIELDS) {
        if (importer[group] != null && Object.keys(importer[group]).length === 0) {
          delete importer[group]
        }
      }
    }
  }
  return changed
}

/**
 * The version resolution would settle on for `alias` under `specifier`: the
 * highest version of it the lockfile already holds that satisfies the range.
 *
 * Resolution prefers a version already in the graph over a higher one from
 * the registry, so reusing what is present is what it would record — for a
 * widened range as much as for one the locked version cannot satisfy at all.
 *
 * `null` when nothing present satisfies (only the resolver can fetch a new
 * version), or when a candidate exists under several peer-suffixed keys,
 * where picking one of them would be a guess.
 */
function highestLockedVersionSatisfying (
  lockfile: LockfileObject,
  alias: string,
  specifier: string
): string | null {
  const versions = new Set<string>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath, snapshot)
    if (name !== alias || nonSemverVersion != null) continue
    if (dp.parseDepPath(depPath).peerDepGraphHash !== '') return null
    if (semver.valid(version) != null && semver.satisfies(version, specifier)) {
      versions.add(version)
    }
  }
  if (versions.size === 0) return null
  return [...versions].sort(semver.rcompare)[0]
}

function dependencyGroupMoved (
  importer: ProjectSnapshot,
  manifest: ProjectManifest,
  alias: string
): boolean {
  const recordedIn = recordedDependencyGroup(importer, alias)
  return recordedIn != null && recordedIn !== effectiveDependencyGroup(manifest, alias)
}

function recordedDependencyGroup (importer: ProjectSnapshot, alias: string): DependenciesField | null {
  return DEPENDENCIES_FIELDS.find((group) => importer[group]?.[alias] != null) ?? null
}

/**
 * The group `satisfiesPackageManifest` expects a manifest dependency to be
 * recorded under when it appears in several: optional wins over prod, prod
 * over dev.
 */
function effectiveDependencyGroup (manifest: ProjectManifest, alias: string): DependenciesField {
  if (manifest.optionalDependencies?.[alias] != null) return 'optionalDependencies'
  if (manifest.dependencies?.[alias] != null) return 'dependencies'
  return 'devDependencies'
}

function getManifestSpecifiers (manifest: ProjectManifest): Record<string, string> {
  return {
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
}
