import type { Config } from '@pnpm/config.reader'
import type { ConfigDependencies, Project, ProjectRootDir } from '@pnpm/types'

export type ProjectsList = Array<Pick<Project, 'rootDir' | 'manifest'>>

export interface WorkspaceState {
  lastValidatedTimestamp: number
  projects: Record<ProjectRootDir, {
    name?: string
    version?: string
    /**
     * Whether a hoisted install left the project with its own modules
     * directory. The hoisted linker creates one only for dependencies it
     * nests under the project, so a project with dependencies may have none;
     * this records which ones must still have it. Never set under the
     * isolated linker, which needs a modules directory for every project with
     * dependencies.
     */
    hasModulesDir?: boolean
  }>
  pnpmfiles: string[]
  filteredInstall: boolean
  configDependencies?: ConfigDependencies
  settings: WorkspaceStateSettings
  /**
   * What the last validation proved about each project. Absent when it
   * recorded nothing, which leaves every project to be proven from the
   * lockfiles.
   */
  validation?: Validation
}

/**
 * The inputs each project was last proven installed against. pnpm v12 writes
 * the same records, so either one can reuse what the other proved.
 */
export interface Validation {
  /**
   * Filesystem time, in nanoseconds since the Unix epoch, by which every
   * entry of `files` was stat'd. A file whose mtime is this close to it may
   * have been rewritten without its mtime moving.
   */
  recordedAtNs: number
  /**
   * The content digest of each input file the projects share (the lockfile,
   * patches, pnpmfiles), keyed by path, with the stat it was read at.
   */
  files?: Record<string, FileSignature>
  /**
   * The digest of the lockfile the projects were proven against, unless
   * `lockfiles` names another one. Absent when there was none.
   */
  lockfile?: string
  /**
   * The digest of the manifest each project was proven installed with, keyed
   * by importer id. A project without an entry is not proven installed.
   */
  manifests: Record<string, string>
  /**
   * The digest of the lockfile a project was proven against, keyed by
   * importer id, where it is not `lockfile`. `null` when there was none.
   */
  lockfiles?: Record<string, string | null>
  /**
   * The digest of every workspace package's bin declarations, keyed by the
   * importer id of each project that was proven installed with them. Only
   * the root and the projects with dependencies link workspace bins, so only
   * they have an entry.
   */
  workspaceBins?: Record<string, string>
}

export interface FileSignature {
  mtimeNs: number
  /** The inode change time, which no tool that restores an mtime can set back. */
  ctimeNs?: number
  size: number
  digest: string
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
  // `node_modules` is linked from the store and virtual store it was
  // installed with, so a change to either must reach the install, which
  // recreates it. Recorded as resolved by `resolveStoreLocationSettings`.
  'storeDir',
  // A `supportedArchitectures` change alters which platform-specific
  // optional dependencies are installed without touching the lockfile,
  // so it must invalidate the up-to-date fast path.
  'supportedArchitectures',
  'trustPolicy',
  'trustPolicyExclude',
  'trustPolicyIgnoreAfter',
  'virtualStoreDir',
  'workspacePackagePatterns',
] as const satisfies ReadonlyArray<keyof Config>

export type WorkspaceStateSettings = Pick<Config, typeof WORKSPACE_STATE_SETTING_KEYS[number]> & {
  /**
   * Recorded by pnpm v12; v11 carries it forward when rewriting the
   * workspace state so a v11 write does not make the next v12 command see
   * a setting change.
   */
  autoDedupe?: boolean
  /**
   * What decides the lockfile keys of registry packages: each prefix
   * `registries` and `namedRegistries` declare, mapped to its URL, and each
   * route (`route:default`, `route:@<scope>`) whose registry declares a
   * prefix, mapped to that prefix. A prefix never contains `:`. Recorded only
   * when a prefix is declared, and only by an install that verified the keys
   * of the lockfile it left behind, so a state written under other
   * registries, by a frozen install of an older lockfile, or by an older pnpm
   * proves nothing about the lockfile's keys.
   */
  registryPrefixes?: Record<string, string>
}
