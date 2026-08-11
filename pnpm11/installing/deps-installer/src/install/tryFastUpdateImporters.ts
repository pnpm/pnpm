import path from 'node:path'

import * as dp from '@pnpm/deps.path'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import { DEPENDENCIES_FIELDS, type DependenciesField, type ProjectId, type ProjectManifest } from '@pnpm/types'
import semver from 'semver'

import { type DroppedEdges, recordDroppedEdge } from './droppedEdges.js'
import type { GraphEdits } from './tryComposeFastUpdates.js'
import { isDirectoryDependency } from './tryFastUpdateSettings.js'

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

export interface FastImportersUpdateOptions {
  projects: Project[]
  pruneLockfileImporters: boolean
  workspacePackages: WorkspacePackages
}

/**
 * Mutates `lockfile` in place and may leave it partially rewritten when it
 * returns `false` — the caller passes the coordinator's disposable candidate,
 * which is discarded on failure. The severed edges and optionality moves are
 * recorded in `edits` for the shared epilogue.
 */
export function tryFastUpdateImporters (
  lockfile: LockfileObject,
  opts: FastImportersUpdateOptions,
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
    const manifestSpecifiers = getManifestSpecifiers(project.manifest)
    if (hasNoRecordedDependencies(importer) && Object.keys(manifestSpecifiers).length > 0) {
      if (!writeImporterFromLockedVersions(lockfile, project, opts.workspacePackages)) return false
      // The only edit that adds reachability, so a package that until now
      // only optional dependencies reached can have stopped being optional.
      edits.optionalFlagsAreStale = true
      changed = true
      continue
    }
    let editedGroups = false
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
        edits.optionalFlagsAreStale = true
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
 * Whether the lockfile records no dependency of this project. That is what a
 * project the lockfile has never seen looks like by the time a handler runs:
 * reading the lockfile seeds every project that has no entry with an empty
 * one, so a new project arrives as an importer with nothing in it.
 */
function hasNoRecordedDependencies (importer: ProjectSnapshot): boolean {
  return Object.keys(importer.specifiers).length === 0 &&
    DEPENDENCIES_FIELDS.every((group) => importer[group] == null || Object.keys(importer[group]).length === 0)
}

/**
 * Write a project's whole importer entry from the versions the lockfile
 * already holds.
 *
 * `false` when a declared dependency needs the resolver: one that resolves to
 * a directory rather than to a registry version, one whose specifier is not a
 * semver range, and one no locked version satisfies.
 */
function writeImporterFromLockedVersions (
  lockfile: LockfileObject,
  project: Project,
  workspacePackages: WorkspacePackages
): boolean {
  const importer: ProjectSnapshot = { specifiers: {} }
  for (const [alias, specifier] of Object.entries(getManifestSpecifiers(project.manifest))) {
    if (isDirectoryDependency(alias, specifier, workspacePackages)) return false
    if (semver.validRange(specifier) == null) return false
    const version = highestLockedVersionSatisfying(lockfile, alias, specifier)
    if (version == null) return false
    const group = effectiveDependencyGroup(project.manifest, alias)
    ;(importer[group] ??= {})[alias] = version
    importer.specifiers[alias] = specifier
  }
  if (project.manifest.dependenciesMeta != null) {
    importer.dependenciesMeta = project.manifest.dependenciesMeta
  }
  if (project.manifest.publishConfig?.directory != null) {
    importer.publishDirectory = project.manifest.publishConfig.directory
  }
  lockfile.importers[project.id] = importer
  return true
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
