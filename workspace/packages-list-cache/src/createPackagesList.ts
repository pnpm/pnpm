import { type PackagesList, type ProjectsList } from './types'

export interface CreatePackagesListOptions {
  allProjects: ProjectsList
  workspaceDir: string
}

export const createPackagesList = ({ allProjects, workspaceDir }: CreatePackagesListOptions): PackagesList => ({
  projectRootDirs: allProjects.map(project => project.rootDir).sort(),
  workspaceDir,
})
