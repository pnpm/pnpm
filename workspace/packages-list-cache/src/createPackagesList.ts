import { type Catalogs } from '@pnpm/catalogs.types'
import { type PackagesList, type ProjectsList } from './types'

export interface CreatePackagesListOptions {
  allProjects: ProjectsList
  catalogs: Catalogs | undefined
  filtered: boolean
  lastValidatedTimestamp: number
}

export const createPackagesList = (opts: CreatePackagesListOptions): PackagesList => ({
  catalogs: opts.catalogs,
  filtered: opts.filtered,
  lastValidatedTimestamp: opts.lastValidatedTimestamp,
  projectRootDirs: opts.allProjects.map(project => project.rootDir).sort(),
})
