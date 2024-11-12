import { type Catalogs } from '@pnpm/catalogs.types'
import { type Project, type ProjectRootDir } from '@pnpm/types'

export type ProjectsList = Array<Pick<Project, 'rootDir'>>

export interface WorkspaceState {
  catalogs: Catalogs | undefined
  lastValidatedTimestamp: number
  projectRootDirs: ProjectRootDir[]
}
