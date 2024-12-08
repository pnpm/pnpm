import { type Catalogs } from '@pnpm/catalogs.types'
import { type WorkspaceState, type ProjectsList } from './types'

export interface CreateWorkspaceStateOptions {
  allProjects: ProjectsList
  catalogs: Catalogs | undefined
  hasPnpmfile: boolean
  linkWorkspacePackages: boolean | 'deep'
  filteredInstall: boolean
}

export const createWorkspaceState = (opts: CreateWorkspaceStateOptions): WorkspaceState => ({
  catalogs: opts.catalogs,
  lastValidatedTimestamp: Date.now(),
  projects: Object.fromEntries(opts.allProjects.map(project => [
    project.rootDir,
    {
      name: project.manifest.name,
      version: project.manifest.version,
    },
  ])),
  hasPnpmfile: opts.hasPnpmfile,
  linkWorkspacePackages: opts.linkWorkspacePackages,
  filteredInstall: opts.filteredInstall,
})
