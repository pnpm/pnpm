import path from 'node:path'

import * as dp from '@pnpm/deps.path'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { DEPENDENCIES_FIELDS, type DependenciesField, type ProjectId, type ProjectManifest } from '@pnpm/types'
import semver from 'semver'

import { type DroppedEdges, recordDroppedEdge } from './droppedEdges.js'
import type { GraphEdits } from './tryComposeFastUpdates.js'

export interface Project {
  id: ProjectId
  manifest: ProjectManifest
}

export function hasChangedProjectSpecifiers (
  lockfile: LockfileObject,
  projects: Project[],
  pruneLockfileImporters: boolean = false
): boolean {
  if (pruneLockfileImporters && staleImporterIds(lockfile, projects).length > 0) return true
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
 * which is discarded on failure. The severed edges and optionality moves are
 * recorded in `edits` for the shared epilogue.
 */
export function tryFastUpdateImporters (
  lockfile: LockfileObject,
  opts: { projects: Project[], pruneLockfileImporters: boolean },
  edits: GraphEdits
): boolean {
  const { projects } = opts
  let changed = false
  if (opts.pruneLockfileImporters) {
    const stale = staleImporterIds(lockfile, projects)
    // A project that is gone while something still links to it is a broken
    // workspace, which only the resolver may report.
    if (stale.some((importerId) => isLinkedFromASurvivor(lockfile, importerId, stale))) {
      return false
    }
    for (const importerId of stale) {
      const importer = lockfile.importers[importerId]
      for (const alias of Object.keys(importer.specifiers)) {
        recordDroppedImporterEdge(edits.dropped, importer, alias)
      }
      delete lockfile.importers[importerId]
      changed = true
    }
  }
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
          // Safe without resolving because the target version is already in
          // the lockfile, subtree and all.
          if (reference !== version) return false
          importer[recordedIn!]![alias] = wanted
          recordDroppedEdge(edits.dropped, alias, reference)
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
      recordDroppedImporterEdge(edits.dropped, importer, alias)
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
 * version), or when the alias appears under a key this cannot turn back
 * into a plain importer reference: a peer-suffixed one, where picking a
 * variant would be a guess, or a registry-qualified one, whose semver only
 * pins a version within its named registry.
 */
function highestLockedVersionSatisfying (
  lockfile: LockfileObject,
  alias: string,
  specifier: string
): string | null {
  const versions = new Set<string>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name, version, nonSemverVersion, registryName } = nameVerFromPkgSnapshot(depPath, snapshot)
    if (name !== alias) continue
    if (nonSemverVersion != null) continue
    if (registryName != null || dp.parseDepPath(depPath).peerDepGraphHash !== '') return null
    if (semver.valid(version) != null && semver.satisfies(version, specifier)) {
      versions.add(version)
    }
  }
  if (versions.size === 0) return null
  return [...versions].sort(semver.rcompare)[0]
}

/** Whether an importer that survives the prune links to `importerId`. */
function isLinkedFromASurvivor (
  lockfile: LockfileObject,
  importerId: ProjectId,
  stale: ProjectId[]
): boolean {
  return Object.entries(lockfile.importers).some(([survivorId, importer]) => {
    if (stale.includes(survivorId as ProjectId)) return false
    return DEPENDENCIES_FIELDS.some((group) =>
      Object.values(importer[group] ?? {}).some((reference) =>
        reference.startsWith('link:') &&
        path.posix.normalize(path.posix.join(survivorId, reference.slice('link:'.length))) === importerId))
  })
}

/** Importers the lockfile records that no project claims any more. */
function staleImporterIds (lockfile: LockfileObject, projects: Project[]): ProjectId[] {
  const projectIds = new Set(projects.map(({ id }) => id))
  return (Object.keys(lockfile.importers) as ProjectId[])
    .filter((importerId) => !projectIds.has(importerId))
}

function dependencyGroupMoved (
  importer: ProjectSnapshot,
  manifest: ProjectManifest,
  alias: string
): boolean {
  const recordedIn = recordedDependencyGroup(importer, alias)
  return recordedIn != null && recordedIn !== effectiveDependencyGroup(manifest, alias)
}

/** Record the edge from `importer` to `alias` as severed. */
function recordDroppedImporterEdge (dropped: DroppedEdges, importer: ProjectSnapshot, alias: string): void {
  const recordedIn = recordedDependencyGroup(importer, alias)
  recordDroppedEdge(dropped, alias, recordedIn == null ? undefined : importer[recordedIn]![alias])
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
