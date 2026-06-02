import type { Config } from '@pnpm/config.reader'
import type { ConfigDependencies, Project, ProjectRootDir } from '@pnpm/types'

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
  'enableGlobalVirtualStore',
  'allowBuilds',
  'autoInstallPeers',
  'catalogs',
  'dedupeDirectDeps',
  'dedupeInjectedDeps',
  'dedupePeerDependents',
  'dedupePeers',
  'dev',
  'excludeLinksFromLockfile',
  'hoistPattern',
  'hoistWorkspacePackages',
  'ignoredOptionalDependencies',
  'injectWorkspacePackages',
  'linkWorkspacePackages',
  // The lockfile-resolution verifier short-circuits on a per-lockfile
  // cache that's keyed by these policy settings; if any of them
  // changes (turning a policy on, shrinking an exclude list, etc.) the
  // workspace state needs to look stale so `optimisticRepeatInstall`
  // doesn't skip the verifier fan-out.
  'minimumReleaseAge',
  'minimumReleaseAgeStrict',
  'minimumReleaseAgeExclude',
  'minimumReleaseAgeIgnoreMissingTime',
  'nodeLinker',
  'optional',
  'overrides',
  'packageExtensions',
  'patchedDependencies',
  'peersSuffixMaxLength',
  'preferWorkspacePackages',
  'production',
  'publicHoistPattern',
  'trustPolicy',
  'trustPolicyExclude',
  'trustPolicyIgnoreAfter',
  'workspacePackagePatterns',
] as const satisfies ReadonlyArray<keyof Config>

export type WorkspaceStateSettings = Pick<Config, typeof WORKSPACE_STATE_SETTING_KEYS[number]>
