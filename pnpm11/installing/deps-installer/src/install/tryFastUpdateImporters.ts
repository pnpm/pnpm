import * as dp from '@pnpm/deps.path'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.types'
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
