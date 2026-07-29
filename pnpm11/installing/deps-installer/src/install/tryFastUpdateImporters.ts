import * as dp from '@pnpm/deps.path'
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
    return Object.entries(getManifestSpecifiers(project.manifest))
      .some(([alias, specifier]) => importer.specifiers[alias] !== specifier)
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
  for (const update of updates) {
    update.specifiers[update.alias] = update.specifier
  }
  return updates.length > 0
}

function getManifestSpecifiers (manifest: ProjectManifest): Record<string, string> {
  return {
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
}
