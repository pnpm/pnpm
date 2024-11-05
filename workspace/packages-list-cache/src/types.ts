import { type Catalogs } from '@pnpm/catalogs.types'
import { type Project, type ProjectRootDir } from '@pnpm/types'

export type ProjectsList = Array<Pick<Project, 'rootDir'>>

export interface PackagesList {
  catalogs?: Catalogs
  lastValidatedTimestamp: number
  projectRootDirs: ProjectRootDir[]
}
