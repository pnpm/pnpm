import path from 'node:path'

import { parse as parseDepPath } from '@pnpm/deps.path'
import type { ProjectId } from '@pnpm/types'

interface ProjectLike {
  id: ProjectId
  rootDir: string
  manifest?: { publishConfig?: { directory?: string, linkDirectory?: boolean } }
}

type ProjectWithTargetDirs<Project> = Project & { id: ProjectId, stages: string[], targetDirs: string[], publishTargetDirs: string[] }

export function extendProjectsWithTargetDirs<Project extends ProjectLike> (
  projects: Array<Project & { id: ProjectId }>,
  injectionTargetsByDepPath: Map<string, string[]>,
  lockfileDir: string
): Array<ProjectWithTargetDirs<Project>> {
  const projectsById: Record<ProjectId, Project & { id: ProjectId, stages?: string[], targetDirs: string[], publishTargetDirs: string[] }> =
    Object.fromEntries(projects.map((project) => [project.id, { ...project, targetDirs: [] as string[], publishTargetDirs: [] as string[] }]))
  const projectsBySourceDir = indexProjectsByPublishDir(projects)

  for (const [depPath, locations] of injectionTargetsByDepPath) {
    const importerId = getInjectedSourceId(depPath)
    if (importerId == null) continue
    // A dep path naming a project's root directly and one naming its publish
    // directory (a `file:` dependency bypasses the `workspace:` protocol's
    // publish-directory redirect) both resolve to the same project, but the
    // resulting copies must stay grouped by which directory they were built
    // from, so each group is later refreshed from its own source.
    const projectById = Object.hasOwn(projectsById, importerId) ? projectsById[importerId] : undefined
    const isPublishDirDepPath = projectById == null
    const project = projectById ?? projectsBySourceDir.get(path.resolve(lockfileDir, importerId))
    if (project == null) continue
    const projectWithTargets = projectsById[project.id]
    addUniqueLocations(isPublishDirDepPath ? projectWithTargets.publishTargetDirs : projectWithTargets.targetDirs, locations)
    projectWithTargets.stages = ['preinstall', 'install', 'postinstall', 'prepare', 'prepublishOnly']
  }

  return Object.values(projectsById) as Array<ProjectWithTargetDirs<Project>>
}

/**
 * A project whose `publishConfig.directory` is injected resolves as its own
 * `file:` dependency path (`a@file:packages/a/dist`), so a dep path may name
 * a project's publish directory rather than any project root.
 */
function indexProjectsByPublishDir<Project extends ProjectLike> (projects: Project[]): Map<string, Project> {
  const projectsBySourceDir = new Map<string, Project>()
  for (const project of projects) {
    const publishDir = project.manifest?.publishConfig?.directory
    if (publishDir == null || project.manifest?.publishConfig?.linkDirectory === false) continue
    const sourceDir = path.resolve(project.rootDir, publishDir)
    if (sourceDir !== project.rootDir) {
      projectsBySourceDir.set(sourceDir, project)
    }
  }
  return projectsBySourceDir
}

function addUniqueLocations (targetGroup: string[], locations: string[]): void {
  for (const location of locations) {
    if (!targetGroup.includes(location)) {
      targetGroup.push(location)
    }
  }
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
