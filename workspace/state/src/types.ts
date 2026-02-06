import { type Config } from '@pnpm/config'
import { type ConfigDependencies, type Project, type ProjectRootDir } from '@pnpm/types'

export type ProjectsList = Array<Pick<Project, 'rootDir' | 'manifest'>>

export interface WorkspaceState {
  lastValidatedTimestamp: number
  projects: Record<ProjectRootDir, {
    name?: string
    version?: string
  }>
  pnpmfiles: string[]
  filteredInstall: boolean
  configDependencies?: ConfigDependencies
  settings: WorkspaceStateSettings
}

export type WorkspaceStateSettings = Pick<Config,
| 'autoInstallPeers'
| 'catalogs'
| 'dedupeDirectDeps'
| 'dedupeInjectedDeps'
| 'dedupePeerDependents'
| 'dev'
| 'excludeLinksFromLockfile'
| 'hoistPattern'
| 'hoistWorkspacePackages'
| 'injectWorkspacePackages'
| 'linkWorkspacePackages'
| 'nodeLinker'
| 'optional'
| 'preferWorkspacePackages'
| 'production'
| 'publicHoistPattern'
| 'workspacePackagePatterns'
>
