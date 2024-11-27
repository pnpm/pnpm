import { type Catalogs } from '@pnpm/catalogs.types'
import { type WorkspaceState, type ProjectsList } from './types'

export interface CreateWorkspaceStateOptions {
  allProjects: ProjectsList
  catalogs: Catalogs | undefined
  lastValidatedTimestamp: number
}

export const createWorkspaceState = (opts: CreateWorkspaceStateOptions): WorkspaceState => ({
  catalogs: opts.catalogs,
  lastValidatedTimestamp: opts.lastValidatedTimestamp,
  projectRootDirs: opts.allProjects.map(project => project.rootDir).sort(),
})
