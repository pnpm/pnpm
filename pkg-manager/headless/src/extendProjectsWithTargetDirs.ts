import { parse as parseDepPath } from '@pnpm/dependency-path'
import { type ProjectId } from '@pnpm/types'

export function extendProjectsWithTargetDirs<T> (
  projects: Array<T & { id: ProjectId }>,
  injectionTargetsByDepPath: Map<string, string[]>
): Array<T & { id: ProjectId, stages: string[], targetDirs: string[] }> {
  const projectsById: Record<ProjectId, T & { id: ProjectId, targetDirs: string[], stages?: string[] }> =
        Object.fromEntries(projects.map((project) => [project.id, { ...project, targetDirs: [] as string[] }]))

  for (const [depPath, locations] of injectionTargetsByDepPath) {
    const parsed = parseDepPath(depPath)
    if (!parsed.name || !parsed.nonSemverVersion?.startsWith('file:')) continue
    const importerId = parsed.nonSemverVersion.replace(/^file:/, '') as ProjectId
    if (projectsById[importerId] == null) continue
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
