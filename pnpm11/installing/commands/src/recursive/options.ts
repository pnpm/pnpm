import type { Catalogs } from '@pnpm/catalogs.types'
import type { CommandHandler } from '@pnpm/cli.command'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import type { DryRunInstallResult, UpdateMatchingFunction } from '@pnpm/installing.deps-installer'
import type { PreferredVersions, ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type { StoreController } from '@pnpm/store.controller'
import type { IncludedDependencies, ProjectRootDir, ProjectsGraph } from '@pnpm/types'

export type RecursiveOptions = CreateStoreControllerOptions & Pick<Config,
| 'bail'
| 'configDependencies'
| 'dedupePeerDependents'
| 'dedupePeers'
| 'depth'
| 'dryRun'
| 'globalPnpmfile'
| 'hoistPattern'
| 'hoistingLimits'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'linkWorkspacePackages'
| 'lockfile'
| 'lockfileDir'
| 'lockfileOnly'
| 'modulesDir'
| 'pnprServer'
| 'allowBuilds'
| 'registriesByScope'
| 'runtime'
| 'save'
| 'saveCatalogName'
| 'saveDev'
| 'saveExact'
| 'saveOptional'
| 'savePeer'
| 'savePrefix'
| 'saveProd'
| 'saveWorkspaceProtocol'
| 'lockfileIncludeTarballUrl'
| 'sharedWorkspaceLockfile'
| 'tag'
| 'trustLockfile'
| 'tryLoadDefaultPnpmfile'
| 'catalogPrune'
| 'minimumReleaseAgeExcludePrune'
| 'trustPolicyExcludePrune'
| 'packageConfigs'
| 'updateConfig'
> & Pick<ConfigContext,
| 'hooks'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> & {
  rebuildHandler?: CommandHandler
  include?: IncludedDependencies
  includeDirect?: IncludedDependencies
  latest?: boolean
  pending?: boolean
  workspace?: boolean
  interactiveUpdate?: boolean
  allowNew?: boolean
  ignoredPackages?: Set<string>
  /**
   * Skip the workspace root project, which a filtered install otherwise
   * installs alongside the selection so that peers resolve from it.
   */
  excludeWorkspaceRootProject?: boolean
  update?: boolean
  updatePackageManifest?: boolean
  updateMatching?: UpdateMatchingFunction
  useBetaCli?: boolean
  allProjectsGraph: ProjectsGraph
  selectedProjectsGraph: ProjectsGraph
  prodAllProjectsGraph?: ProjectsGraph
  prodOnlySelectedProjectDirs?: ProjectRootDir[]
  preferredVersions?: PreferredVersions
  preferredVersionsByImporterId?: Record<string, PreferredVersions>
  pruneDirectDependencies?: boolean
  pruneLockfileImporters?: boolean
  storeControllerAndDir?: {
    ctrl: StoreController
    dir: string
    resolutionVerifiers: ResolutionVerifier[]
  }
  pnpmfile: string[]
  /**
   * Alternative install engine (today: pacquet) the deps-installer
   * delegates the install to. Built in `installDeps` when
   * `configDependencies.pacquet` is declared, threaded through here so
   * the recursive workspace path picks it up too.
   */
  runPacquet?: {
    supportsResolution: boolean
    run: (opts?: { filterResolvedProgress?: boolean, resolve?: boolean }) => Promise<void>
  }
} & Partial<
  Pick<Config,
| 'ci'
| 'sort'
| 'strictDepBuilds'
| 'useGitBranchLockfile'
| 'mergeGitBranchLockfiles'
| 'workspaceConcurrency'
  >
> & Required<
  Pick<Config, 'workspaceDir'>
>

export type CommandFullName = 'install' | 'add' | 'remove' | 'update' | 'import'

export interface RecursiveResult {
  passed: boolean | string
  /**
   * Catalog entries written to `pnpm-workspace.yaml` during this install.
   * The caller folds these into the catalogs recorded in the workspace state
   * cache so that reverting a catalog entry is detected as an outdated state.
   */
  updatedCatalogs?: Catalogs
  /**
   * Present only for a `dryRun` install over a shared workspace lockfile:
   * the before/after wanted lockfiles for the caller to diff.
   */
  dryRunResult?: DryRunInstallResult
}
