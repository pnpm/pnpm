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

export const WORKSPACE_STATE_SETTING_KEYS = [
  'autoInstallPeers',
  'catalogs',
  'dedupeDirectDeps',
  'dedupeInjectedDeps',
  'dedupePeerDependents',
  'dev',
  'excludeLinksFromLockfile',
  'hoistPattern',
  'hoistWorkspacePackages',
  'ignoredOptionalDependencies',
  'injectWorkspacePackages',
  'linkWorkspacePackages',
  'nodeLinker',
  'optional',
  'overrides',
  'packageExtensions',
  'patchedDependencies',
  'peersSuffixMaxLength',
  'preferWorkspacePackages',
  'production',
  'publicHoistPattern',
  'workspacePackagePatterns',
] as const satisfies ReadonlyArray<keyof Config>

export type WorkspaceStateSettings = Pick<Config, typeof WORKSPACE_STATE_SETTING_KEYS[number]>
