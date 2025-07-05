import pick from 'ramda/src/pick'
import { type WorkspaceState, type WorkspaceStateSettings, type ProjectsList } from './types'

export interface CreateWorkspaceStateOptions {
  allProjects: ProjectsList
  pnpmfiles: string[]
  filteredInstall: boolean
  settings: WorkspaceStateSettings
  configDependencies?: Record<string, string>
}

export const createWorkspaceState = (opts: CreateWorkspaceStateOptions): WorkspaceState => ({
  lastValidatedTimestamp: Date.now(),
  projects: Object.fromEntries(opts.allProjects.map(project => [
    project.rootDir,
    {
      name: project.manifest.name,
      version: project.manifest.version,
    },
  ])),
  pnpmfiles: opts.pnpmfiles,
  settings: pick([
    'autoInstallPeers',
    'catalogs',
    'dedupeDirectDeps',
    'dedupeInjectedDeps',
    'dedupePeerDependents',
    'dev',
    'excludeLinksFromLockfile',
    'hoistPattern',
    'hoistWorkspacePackages',
    'injectWorkspacePackages',
    'linkWorkspacePackages',
    'nodeLinker',
    'optional',
    'preferWorkspacePackages',
    'production',
    'publicHoistPattern',
    'workspacePackagePatterns',
  ], opts.settings),
  filteredInstall: opts.filteredInstall,
  configDependencies: opts.configDependencies,
})
