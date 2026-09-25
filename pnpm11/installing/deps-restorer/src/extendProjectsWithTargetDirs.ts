import path from 'node:path'

import { parse as parseDepPath } from '@pnpm/deps.path'
import type { ProjectId } from '@pnpm/types'

interface ProjectLike {
  id: ProjectId
  rootDir: string
  manifest?: { publishConfig?: { directory?: string, linkDirectory?: boolean } }
}

export function extendProjectsWithTargetDirs<T extends ProjectLike> (
  projects: Array<T & { id: ProjectId }>,
  injectionTargetsByDepPath: Map<string, string[]>,
  lockfileDir: string
): Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }> {
  const projectsById: Record<ProjectId, T & { id: ProjectId, stages?: string[], targetDirs: string[] }> =
    Object.fromEntries(projects.map((project) => [project.id, { ...project, targetDirs: [] as string[] }]))

  // A project whose `publishConfig.directory` is injected resolves as its own
  // `file:` dependency path (`a@file:packages/a/dist`), so a dep path may name
  // a project's publish directory rather than any project root.
  const projectsBySourceDir = new Map<string, T & { id: ProjectId }>()
  for (const project of projects) {
    const publishDir = project.manifest?.publishConfig?.directory
    if (publishDir == null || project.manifest?.publishConfig?.linkDirectory === false) continue
    const sourceDir = path.resolve(project.rootDir, publishDir)
    if (sourceDir !== project.rootDir) {
      projectsBySourceDir.set(sourceDir, project)
    }
  }

  for (const [depPath, locations] of injectionTargetsByDepPath) {
    const importerId = getInjectedSourceId(depPath)
    if (importerId == null) continue
    const project = projectsById[importerId] ?? projectsBySourceDir.get(path.resolve(lockfileDir, importerId))
    if (project == null) continue
    const projectWithTargets = projectsById[project.id]
    // Dedupe: only add locations that aren't already tracked
    for (const location of locations) {
      if (!projectWithTargets.targetDirs.includes(location)) {
        projectWithTargets.targetDirs.push(location)
      }
    }
    projectWithTargets.stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
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
