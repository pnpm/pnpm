import { Project } from '@pnpm/headless'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'

export async function toAllProjects (projects: Array<Omit<Project, 'buildIndex' | 'manifest'>>) {
  return Promise.all(
    projects.map(async (project, buildIndex) => ({
      ...project,
      manifest: await readPackageJsonFromDir(project.rootDir),
      buildIndex,
    }))
  )
}

export function toAllProjectsMap (allProjects: Project[]) {
  const allProjectsMap = new Map<string, Omit<Project, 'buildIndex'>>()
  for (const project of allProjects) {
    allProjectsMap.set(project.id, project)
  }
  return allProjectsMap
}
