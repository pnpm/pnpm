import type { ConfigDependencies } from '@pnpm/types'
import { pick } from 'ramda'

import { type ProjectsList, WORKSPACE_STATE_SETTING_KEYS, type WorkspaceState, type WorkspaceStateSettings } from './types.js'

export interface CreateWorkspaceStateOptions {
  allProjects: ProjectsList
  pnpmfiles: string[]
  filteredInstall: boolean
  settings: WorkspaceStateSettings
  configDependencies?: ConfigDependencies
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
  settings: pick(WORKSPACE_STATE_SETTING_KEYS, opts.settings),
  filteredInstall: opts.filteredInstall,
  configDependencies: opts.configDependencies,
})
