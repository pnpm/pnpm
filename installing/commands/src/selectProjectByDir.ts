import path from 'node:path'

import type { Project, ProjectsGraph } from '@pnpm/types'

export function selectProjectByDir (projects: Project[], searchedDir: string): ProjectsGraph | undefined {
  const project = projects.find(({ rootDir }) => path.relative(rootDir, searchedDir) === '')
  if (project == null) return undefined
  return { [project.rootDir]: { dependencies: [], package: project } }
}
