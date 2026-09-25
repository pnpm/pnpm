import path from 'node:path'

import { parse as parseDepPath } from '@pnpm/deps.path'
import type { ProjectId } from '@pnpm/types'

export function extendProjectsWithTargetDirs<T> (
  projects: Array<T & { id: ProjectId }>,
  injectionTargetsByDepPath: Map<string, string[]>
): Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }> {
  const projectsById: Record<ProjectId, T & { id: ProjectId, targetDirs: string[], stages?: string[] }> =
    Object.fromEntries(projects.map((project) => [project.id, { ...project, targetDirs: [] as string[] }]))

  for (const [depPath, locations] of injectionTargetsByDepPath) {
    const importerId = getInjectedSourceId(depPath)
    if (importerId == null || projectsById[importerId] == null) continue
    // Dedupe: only add locations that aren't already tracked
    for (const location of locations) {
      if (!projectsById[importerId].targetDirs.includes(location)) {
        projectsById[importerId].targetDirs.push(location)
      }
    }
    projectsById[importerId].stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
  }

  return Object.values(projectsById) as Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }>
}

/**
 * The `injectedDeps` field of `.modules.yaml`: the injected copies of every
 * directory dependency, keyed by the source directory. Keys and copies are
 * relative to `lockfileDir`. A source outside the lockfile's projects is
 * listed too, as a project with its own lockfile injects its dependencies
 * from other workspace projects.
 */
export function getInjectedDeps (
  injectionTargetsByDepPath: Map<string, string[]>,
  lockfileDir: string
): Record<string, string[]> {
  const injectedDeps: Record<string, string[]> = Object.create(null)
  for (const [depPath, locations] of injectionTargetsByDepPath) {
    const sourceId = getInjectedSourceId(depPath)
    if (sourceId == null || locations.length === 0) continue
    injectedDeps[sourceId] ??= []
    for (const location of locations) {
      const targetDir = path.relative(lockfileDir, location)
      if (!injectedDeps[sourceId].includes(targetDir)) {
        injectedDeps[sourceId].push(targetDir)
      }
    }
  }
  return injectedDeps
}

function getInjectedSourceId (depPath: string): ProjectId | undefined {
  const parsed = parseDepPath(depPath)
  if (!parsed.name || !parsed.nonSemverVersion?.startsWith('file:')) return undefined
  return parsed.nonSemverVersion.replace(/^file:/, '') as ProjectId
}
