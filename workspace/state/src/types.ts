import { type Catalogs } from '@pnpm/catalogs.types'
import { type Project, type ProjectRootDir } from '@pnpm/types'

export type ProjectsList = Array<Pick<Project, 'rootDir' | 'manifest'>>

export interface WorkspaceState {
  catalogs: Catalogs | undefined
  lastValidatedTimestamp: number
  projects: Record<ProjectRootDir, {
    name?: string
    version?: string
  }>
  hasPnpmfile: boolean
  linkWorkspacePackages: boolean | 'deep'
  filteredInstall: boolean
}
