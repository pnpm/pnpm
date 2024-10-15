import { type Catalogs } from '@pnpm/catalogs.types'
import { type PackagesList, type ProjectsList } from './types'

export interface CreatePackagesListOptions {
  allProjects: ProjectsList
  catalogs?: Catalogs
  lastValidatedTimestamp: number
  workspaceDir: string
}

export const createPackagesList = (opts: CreatePackagesListOptions): PackagesList => ({
  catalogs: opts.catalogs,
  lastValidatedTimestamp: opts.lastValidatedTimestamp,
  projectRootDirs: opts.allProjects.map(project => project.rootDir).sort(),
  workspaceDir: opts.workspaceDir,
})
