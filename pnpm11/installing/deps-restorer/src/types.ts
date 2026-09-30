import type { HoistingLimits } from '@pnpm/installing.linking.real-hoist'
import type { IncludedDependencies, Modules } from '@pnpm/installing.modules-yaml'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { LogBase } from '@pnpm/logger'
import type { PatchGroupRecord } from '@pnpm/patching.config'
import type { StoreController } from '@pnpm/store.controller-types'
import type {
  DepPath,
  HoistedDependencies,
  IgnoredBuilds,
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
  RegistryConfig,
  RegistryContext,
  RemoteSideEffectsCacheSettings,
  SupportedArchitectures,
} from '@pnpm/types'

export type ReporterFunction = (logObj: LogBase) => void

export interface Project {
  binsDir: string
  buildIndex: number
  manifest: ProjectManifest
  modulesDir: string
  id: ProjectId
  pruneDirectDependencies?: boolean
  rootDir: ProjectRootDir
}

export interface HeadlessOptions extends RegistryContext {
  projectDependencies?: Map<ProjectRootDir, ProjectRootDir[]>
  resolvePeersFromWorkspaceRoot?: boolean
  allowBuilds?: Record<string, boolean | string>
  autoInstallPeers?: boolean
  childConcurrency?: number
  currentLockfile?: LockfileObject
  currentEngine: {
    nodeVersion?: string
    pnpmVersion: string
  }
  /** `true` when `currentEngine.nodeVersion` is not configured by the user. */
  nodeVersionFromEnginesRuntime?: boolean
  dedupeDirectDeps?: boolean
  enablePnp?: boolean
  engineStrict: boolean
  /** See {@link LockfileToDepGraphOptions.omitResolvedProgress}. */
  omitResolvedProgress?: boolean
  /**
   * Skip the `pnpm:summary` log this install would emit. The default reporter
   * renders the first summary event it sees, so a caller that runs several
   * installs and emits one consolidated summary of its own has to keep each
   * of them quiet. `pnpm add -g` and `pnpm update -g` do exactly that.
   */
  omitSummaryLog?: boolean
  excludeLinksFromLockfile?: boolean
  extraBinPaths?: string[]
  extraEnv?: Record<string, string>
  extendNodePath?: boolean
  extraNodePaths?: string[]
  preferSymlinkedExecutables?: boolean
  hoistingLimits?: HoistingLimits
  externalDependencies?: Set<string>
  ignoreScripts: boolean
  deferDependencyBuilds?: boolean
  ignorePackageManifest?: boolean
  /**
   * When true, skip fetching local dependencies (file: protocol pointing to directories).
   * This is used by `pnpm fetch` which only downloads packages from the registry
   * and doesn't need local packages that won't be available (e.g., in Docker builds).
   */
  ignoreLocalPackages?: boolean
  deploy?: boolean
  include: IncludedDependencies
  selectedProjectDirs: string[]
  /**
   * The selected projects whose own install stages may run. Defaults to
   * every selected project; an `uninstallSome` mutation materializes for its
   * project without running them, matching the resolution path.
   */
  projectDirsRunningScripts?: string[]
  /**
   * The root project's `preinstall` already ran, ahead of resolution, so its
   * lifecycle scripts here start at `install`.
   */
  rootProjectPreinstallRan?: boolean
  /** The selected projects that run `postuninstall` in place of the install stages. */
  projectDirsRunningUninstallScripts?: string[]
  /** The selected projects that run only install stages without prepare. */
  projectDirsRunningInstallOnlyScripts?: string[]
  allProjects: Record<string, Project>
  prunedAt?: string
  hoistedDependencies: HoistedDependencies
  hoistPattern?: string[]
  publicHoistPattern?: string[]
  currentHoistPattern?: string[]
  currentPublicHoistPattern?: string[]
  currentHoistedLocations?: Record<string, string[]>
  lockfileDir: string
  modulesDir?: string
  enableGlobalVirtualStore?: boolean
  globalVirtualStoreDir: string
  virtualStoreDir?: string
  virtualStoreDirMaxLength: number
  patchedDependencies?: PatchGroupRecord
  scriptsPrependNodePath?: boolean | 'warn-only'
  scriptShell?: string
  shellEmulator?: boolean
  storeController: StoreController
  sideEffectsCacheRead: boolean
  sideEffectsCacheWrite: boolean
  remoteSideEffectsCache?: RemoteSideEffectsCacheSettings
  pnprServer?: string
  symlink?: boolean
  disableRelinkLocalDirDeps?: boolean
  force: boolean
  /** See `installabilityUnderForce` in `@pnpm/config.package-is-installable`. */
  forceIgnoresPlatform?: boolean
  storeDir: string
  configByUri: Record<string, RegistryConfig>
  unsafePerm: boolean
  userAgent: string
  reporter?: ReporterFunction
  packageManager: {
    name: string
    version: string
  }
  pruneStore: boolean
  pruneVirtualStore?: boolean
  wantedLockfile?: LockfileObject
  ownLifecycleHooksStdio?: 'inherit' | 'pipe'
  pendingBuilds: string[]
  relinkChangedDependenciesOnly?: boolean
  resolveSymlinksInInjectedDirs?: boolean
  skipped: Set<DepPath>
  skipRuntimes?: boolean
  enableModulesDir?: boolean
  virtualStoreOnly?: boolean
  nodeExperimentalPackageMap?: boolean
  nodePackageMapType?: 'standard' | 'loose'
  nodeLinker?: 'isolated' | 'hoisted' | 'pnp'
  useGitBranchLockfile?: boolean
  useLockfile?: boolean
  supportedArchitectures?: SupportedArchitectures
  hoistWorkspacePackages?: boolean
  modulesFile?: Modules | null
  /**
   * Awaited right before dependency lifecycle scripts run. Lets the caller
   * overlap lockfile verification with fetching and linking while still
   * guaranteeing no dependency script executes on an unverified lockfile:
   * the returned promise rejects when verification failed.
   */
  verifyLockfile?: () => Promise<void>
}

export interface InstallationResultStats {
  added: number
  removed: number
  linkedToRoot: number
}

export interface InstallationResult {
  stats: InstallationResultStats
  ignoredBuilds: IgnoredBuilds | undefined
}
