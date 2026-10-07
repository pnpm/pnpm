import type { Catalogs } from '@pnpm/catalogs.types'
import type { RunLifecycleHooksConcurrentlyOptions } from '@pnpm/exec.lifecycle'
import type { OverriddenDependencyMatcher } from '@pnpm/hooks.read-package-hook'
import type { PnpmContext } from '@pnpm/installing.context'
import type { RangeSpecStyle, UpdateMatchingFunction, WantedDependency } from '@pnpm/installing.deps-resolver'
import type { InstallationResultStats } from '@pnpm/installing.deps-restorer'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { PatchGroupRecord } from '@pnpm/patching.config'
import type { PreferredVersions, ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type {
  AllowBuild,
  DependenciesField,
  DepPath,
  IgnoredBuilds,
  PeerDependencyIssues,
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
  ReadPackageHook,
} from '@pnpm/types'

import type { InstallOptions, ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'

export interface InstallMutationOptions {
  update?: boolean
  updatePatches?: boolean
  updateToLatest?: boolean
  updateMatching?: UpdateMatchingFunction
  updatePackageManifest?: boolean
}

export interface InstallDepsMutation extends InstallMutationOptions {
  mutation: 'install'
  pruneDirectDependencies?: boolean
}

export interface InstallSomeDepsMutation extends InstallMutationOptions {
  allowNew?: boolean
  dependencySelectors: string[]
  mutation: 'installSome'
  peer?: boolean
  peerAliases?: Set<string>
  savePeer?: boolean
  pruneDirectDependencies?: boolean
  rangeSpecStyle?: RangeSpecStyle
  targetDependenciesField?: DependenciesField
}

export interface UninstallSomeDepsMutation {
  mutation: 'uninstallSome'
  dependencyNames: string[]
  targetDependenciesField?: DependenciesField
}

export type DependenciesMutation = InstallDepsMutation | InstallSomeDepsMutation | UninstallSomeDepsMutation

export type SingleProjectInstallOptions = Omit<InstallOptions, 'allProjects'> & {
  preferredVersions?: PreferredVersions
  pruneDirectDependencies?: boolean
  binsDir?: string
} & InstallMutationOptions

export interface InstallResult {
  /**
   * A partial of new or updated catalog config entries. A change will be
   * produced if a dependency using the catalog protocol was newly added or
   * updated during this install. To obtain the full catalog, callers should
   * merge this object with the current catalog configs in pnpm-workspace.yaml.
   */
  updatedCatalogs: Catalogs | undefined
  updatedManifest: ProjectManifest
  ignoredBuilds: IgnoredBuilds | undefined
  /** Forwarded from {@link MutateModulesResult.newLockfile}. */
  newLockfile?: LockfileObject
  /** Forwarded from {@link MutateModulesResult.resolutionPolicyViolations}. */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  /** Forwarded from {@link MutateModulesResult.dryRunResult}. */
  dryRunResult?: DryRunInstallResult
}

export interface ProjectToBeInstalled {
  id: string
  buildIndex: number
  manifest: ProjectManifest
  modulesDir: string
  rootDir: ProjectRootDir
  stages?: string[]
}

export type MutatedProject = DependenciesMutation & { rootDir: ProjectRootDir }

export type MutateModulesOptions = InstallOptions & {
  preferredVersions?: PreferredVersions
  preferredVersionsByImporterId?: Record<string, PreferredVersions>
  hooks?: {
    readPackage?: ReadPackageHook[] | ReadPackageHook
  } | InstallOptions['hooks']
}

export interface MutateModulesInSingleProjectResult {
  updatedCatalogs: Catalogs | undefined
  updatedProject: UpdatedProject
  ignoredBuilds: IgnoredBuilds | undefined
  /** Forwarded from {@link MutateModulesResult.newLockfile}. */
  newLockfile?: LockfileObject
  /** Forwarded from {@link MutateModulesResult.wantedLockfile}. */
  wantedLockfile?: LockfileObject
  /** Forwarded from {@link MutateModulesResult.resolutionPolicyViolations}. */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  /** Forwarded from {@link MutateModulesResult.dryRunResult}. */
  dryRunResult?: DryRunInstallResult
}

export interface MutateModulesResult {
  updatedCatalogs?: Catalogs
  updatedProjects: UpdatedProject[]
  /**
   * The wanted lockfile as freshly resolved by this mutation. Absent when
   * no new resolution ran (e.g. a frozen install reused the existing
   * lockfile) or resolution was delegated (pnpr server).
   */
  newLockfile?: LockfileObject
  /**
   * The existing wanted lockfile, used as is. Present only when
   * {@link newLockfile} is absent because resolution was skipped.
   */
  wantedLockfile?: LockfileObject
  stats: InstallationResultStats
  depsRequiringBuild?: DepPath[]
  ignoredBuilds: IgnoredBuilds | undefined
  /**
   * Resolver-policy violations the post-resolution scan found in the
   * freshly-resolved lockfile. Empty array when no verifier reported a
   * violation or no policy was active.
   */
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  /**
   * Present only for a `dryRun` install: the before/after wanted lockfiles
   * the resolve produced without writing, for the caller to diff.
   */
  dryRunResult?: DryRunInstallResult
}

export interface InnerInstallResult {
  readonly updatedCatalogs?: Catalogs
  readonly updatedProjects: UpdatedProject[]
  readonly newLockfile?: LockfileObject
  readonly wantedLockfile?: LockfileObject
  readonly stats?: InstallationResultStats
  readonly depsRequiringBuild?: DepPath[]
  readonly ignoredBuilds: IgnoredBuilds | undefined
  readonly dryRunResult?: DryRunInstallResult
  readonly resolutionPolicyViolations?: ResolutionPolicyViolation[]
}

export type ImporterToUpdate = {
  buildIndex: number
  binsDir: string
  id: ProjectId
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  isOverriddenDependency?: OverriddenDependencyMatcher
  modulesDir: string
  rootDir: ProjectRootDir
  pruneDirectDependencies: boolean
  removePackages?: string[]
  updatePackageManifest: boolean
  wantedDependencies: WantedDependency[]
} & DependenciesMutation

export interface UpdatedProject {
  originalManifest?: ProjectManifest
  manifest: ProjectManifest
  peerDependencyIssues?: PeerDependencyIssues
  rootDir: ProjectRootDir
}

/**
 * The before/after wanted lockfiles a `dryRun` install resolved without
 * writing. The caller diffs them to report what a real install would change.
 */
export interface DryRunInstallResult {
  originalLockfile: LockfileObject
  wantedLockfile: LockfileObject
}

export interface InstallFunctionResult {
  updatedCatalogs?: Catalogs
  newLockfile: LockfileObject
  projects: UpdatedProject[]
  stats?: InstallationResultStats
  depsRequiringBuild: DepPath[]
  ignoredBuilds?: IgnoredBuilds
  resolutionPolicyViolations: ResolutionPolicyViolation[]
  dryRunResult?: DryRunInstallResult
}

export type InstallInContextOptions = Omit<StrictInstallOptions, 'patchedDependencies'> & {
  allowBuild?: AllowBuild
  patchedDependencies?: PatchGroupRecord
  makePartialCurrentLockfile: boolean
  needsFullResolution: boolean
  overrides?: Record<string, string>
  staleOverrideTargets?: ReadonlySet<string>
  updateLockfileMinorVersion: boolean
  preferredVersions?: PreferredVersions
  preferredVersionsByImporterId?: Record<string, PreferredVersions>
  pruneVirtualStore: boolean
  /** The root project's `preinstall` already ran, ahead of resolution. */
  rootProjectPreinstallRan: boolean
  projectDirsRemovingDeps: Set<ProjectRootDir>
  scriptsOpts: RunLifecycleHooksConcurrentlyOptions
  currentLockfileIsUpToDate: boolean
  hoistWorkspacePackages?: boolean
  verifyLockfile?: () => Promise<void>
}

export type InstallFunction = (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: InstallInContextOptions
) => Promise<InstallFunctionResult>

export interface MutationRunBase {
  projects: MutatedProject[]
  maybeOpts: MutateModulesOptions
  opts: StrictInstallOptions
  detachReporter: () => void
}

/**
 * The state `mutateModules` settles before it installs, shared by the steps
 * of the install.
 */
export interface MutationRun extends MutationRunBase {
  ctx: PnpmContext
  allowBuild: AllowBuild | undefined
  installsOnly: boolean
  /**
   * Removals, and additions the lockfile already holds a version for, may
   * take the fast lockfile update and the frozen-like install; an explicitly
   * frozen run keeps its stricter behavior. An addition also has to have had
   * its manifest edit committed, which only a rewrite that passed its gates
   * does — see `addedManifests`.
   */
  installsAndUninstallsOnly: boolean
  scriptsOpts: RunLifecycleHooksConcurrentlyOptions
  rootProjectPreinstallRan: boolean
  verifyLockfilePromise?: Promise<void>
  /**
   * Gate passed down to the build phase: fetching and linking overlap with
   * verification, but no dependency lifecycle script may run until the verdict
   * is in. Awaiting the promise throws if verification failed, aborting
   * before any script executes. `settleInstall` is the catch-all that still
   * reconciles the verdict on paths that never reach the build phase.
   */
  verifyLockfile?: () => Promise<void>
  forceResolutionFromHook: boolean
  pruneVirtualStore: boolean
}
