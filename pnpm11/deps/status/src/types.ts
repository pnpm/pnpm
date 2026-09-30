import type { Config, ConfigContext } from '@pnpm/config.reader'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { IncludedDependencies } from '@pnpm/types'
import type { WorkspaceState, WorkspaceStateSettings } from '@pnpm/workspace.state'

export type CheckDepsStatusOptions = Pick<Config,
| 'autoInstallPeers'
| 'catalogs'
| 'dedupeDirectDeps'
| 'excludeLinksFromLockfile'
| 'ignorePnpmfile'
| 'injectWorkspacePackages'
| 'linkWorkspacePackages'
| 'lockfileDir'
| 'mergeGitBranchLockfiles'
| 'modulesDir'
| 'modulesDirsByProjectName'
| 'nodeLinker'
| 'packageConfigs'
| 'patchedDependencies'
| 'peersSuffixMaxLength'
| 'sharedWorkspaceLockfile'
| 'workspaceDir'
| 'patchesDir'
| 'configDependencies'
| 'overrides'
| 'packageExtensions'
| 'ignoredOptionalDependencies'
> & Pick<ConfigContext,
| 'allProjects'
| 'hooks'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'selectedProjectsGraph'
> & {
  /**
   * The project a non-recursive command runs in. When `selectedProjectsGraph`
   * is absent, it is the project held to the modules-directory requirement
   * after a filtered install.
   */
  dir?: string
  ignoreFilteredInstallCache?: boolean
  ignoredWorkspaceStateSettings?: Array<keyof WorkspaceStateSettings>
  pnpmfile: string[]
  /**
   * The checks below only track manifest and lockfile mtimes, so edits inside
   * a local file dependency's directory (or a repacked local tarball) go
   * unnoticed. Callers that skip the install entirely when this check reports
   * up-to-date must set this so that projects with local file dependencies
   * (`file:` and bare local path/tarball specifiers) always run a real
   * install, which refetches those dependencies
   * (https://github.com/pnpm/pnpm/issues/11795).
   */
  treatLocalFileDepsAsOutdated?: boolean
  /**
   * Which dependency groups the current install materializes. Local file
   * dependencies in an excluded group (for example `devDependencies` under
   * `--prod`) are not installed, so they don't force the
   * `treatLocalFileDepsAsOutdated` bail-out. A change to these flags between
   * installs is caught separately by the workspace state settings comparison
   * (`dev`/`optional`/`production` are part of
   * `WORKSPACE_STATE_SETTING_KEYS`).
   */
  include?: IncludedDependencies
  /**
   * When git-branch lockfiles are enabled, the wanted lockfile lives at
   * `pnpm-lock.<branch>.yaml`, so a missing `pnpm-lock.yaml` is the steady
   * state — the current-lockfile stand-in must not kick in.
   */
  useGitBranchLockfile?: boolean
} & WorkspaceStateSettings

export interface CheckDepsStatusResult {
  upToDate: boolean | undefined
  issue?: string
  workspaceState: WorkspaceState | undefined
  /**
   * Set when `pnpm-lock.yaml` was missing and the current lockfile
   * (`<lockfileDir>/node_modules/.pnpm/lock.yaml`) stood in as the wanted
   * lockfile for the up-to-date checks. The current lockfile records
   * exactly what the previous install materialized, so the caller can
   * restore `pnpm-lock.yaml` from it without resolving — `installDeps`
   * does that before reporting "Already up to date".
   */
  wantedLockfileToRestore?: {
    lockfile: LockfileObject
    lockfileDir: string
  }
}
