import { buildProjects, PROJECT_INSTALL_STAGES } from '@pnpm/building.after-install'
import type { Catalogs } from '@pnpm/catalogs.types'
import { readProjectManifestOnly, tryReadProjectManifest } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import {
  type DryRunInstallResult,
  install,
  mutateModulesInSingleProject,
  type MutateModulesOptions,
  type UpdateMatchingFunction,
  type WorkspacePackages,
} from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { applyRuntimeOnFailOverride, filterDependenciesByType, getRangeSpecStyle } from '@pnpm/pkg-manifest.utils'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type { createStoreController } from '@pnpm/store.connection-manager'
import type {
  IncludedDependencies,
  Project,
  ProjectManifest,
  ProjectRootDir,
} from '@pnpm/types'
import { filterProjectsBySelectorObjects } from '@pnpm/workspace.projects-filter'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'

import { getSaveType } from './getSaveType.js'
import { handleIgnoredBuilds } from './handleIgnoredBuilds.js'
import type { InstallDepsOptions } from './installDeps.js'
import { type PolicyHandlersPlan, setupPolicyHandlers } from './policyHandlers.js'
import {
  createMatcher,
  failOnVersionsOfIndirectUpdateSpecs,
  makeIgnorePatterns,
  matchDependencies,
  type UpdateDepsMatcher,
} from './recursive.js'
import { keptCatalogsForPrune, resolvedPackageVersionsForPrune } from './resolvedPackageVersionsForPrune.js'
import type { makeRunPacquet } from './runPacquet.js'
import { toWorkspaceSpecs } from './updateWorkspaceDependencies.js'
import { createVulnerabilityUpdateMatching, preferNonvulnerablePackageVersions } from './vulnerabilityPreferences.js'
import { recursiveInstallThenUpdateWorkspaceState, updateSingleProjectWorkspaceState } from './workspaceStateUpdate.js'

const OVERWRITE_UPDATE_OPTIONS = {
  allowNew: true,
  update: false,
}

type StoreControllerAndDir = Awaited<ReturnType<typeof createStoreController>>
type WriteProjectManifest = Awaited<ReturnType<typeof tryReadProjectManifest>>['writeProjectManifest']
type CoreInstallDepsOptions = Omit<InstallDepsOptions, 'reporter'>

export interface SingleProjectInstallTarget {
  opts: InstallDepsOptions
  allProjects: Project[]
  store: StoreControllerAndDir
  runPacquet: ReturnType<typeof makeRunPacquet> | undefined
  includeDirect: IncludedDependencies
}

interface SingleProjectInstallContext extends SingleProjectInstallTarget {
  coreOpts: CoreInstallDepsOptions
  installOpts: Omit<MutateModulesOptions, 'allProjects'>
  manifest: ProjectManifest
  writeProjectManifest: WriteProjectManifest
  policyHandlers: PolicyHandlersPlan | undefined
  workspacePackages: WorkspacePackages
  /**
   * `params` is rewritten into the dependency names it matched, so this
   * remembers whether the user named any package. `--workspace` only insists
   * that a dependency exists in the workspace when it was asked for by name.
   */
  userNamedDeps: boolean
}

interface DependencySelection {
  params: string[]
  updatePackageManifest?: boolean
  updateMatching?: UpdateMatchingFunction
}

export async function installSingleProject (
  target: SingleProjectInstallTarget,
  requestedParams: string[]
): Promise<DryRunInstallResult | undefined> {
  // `pnpm install ""` is going to be just `pnpm install`
  const params = requestedParams.filter(Boolean)
  const ctx = await createSingleProjectInstallContext(target, params.length > 0)
  const selection = selectUpdateTargets(ctx, params)
  if (selection == null) return undefined
  const dependencySelectors = completeDependencySelectors(ctx, selection.params)
  if (dependencySelectors.length) {
    return installSomeDependencies(ctx, dependencySelectors)
  }
  return installAllDependencies(ctx, selection)
}

async function createSingleProjectInstallContext (
  target: SingleProjectInstallTarget,
  userNamedDeps: boolean
): Promise<SingleProjectInstallContext> {
  const { opts, allProjects, store, runPacquet } = target
  let workspacePackages!: WorkspacePackages

  if (opts.workspaceDir) {
    workspacePackages = arrayOfWorkspacePackagesToMap(allProjects) as WorkspacePackages
  }
  const { manifest, writeProjectManifest } = await readManifestToInstall(opts, userNamedDeps)

  const policyHandlers = setupPolicyHandlers(opts)

  const { reporter: reporterName, ...coreOpts } = opts

  const installOpts: Omit<MutateModulesOptions, 'allProjects'> = {
    ...coreOpts,
    // In case installation is done in a multi-package repository
    // The dependencies should be built first,
    // so ignoring scripts for now
    ignoreScripts: !!workspacePackages || opts.ignoreScripts,
    linkWorkspacePackagesDepth: opts.linkWorkspacePackages === 'deep' ? Infinity : opts.linkWorkspacePackages ? 0 : -1,
    sideEffectsCacheRead: opts.sideEffectsCacheRead,
    sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
    skipRuntimes: opts.runtime === false,
    storeController: store.ctrl,
    storeDir: store.dir,
    resolutionVerifiers: store.resolutionVerifiers,
    workspacePackages,
    preferredVersions: opts.packageVulnerabilityAudit ? preferNonvulnerablePackageVersions(opts.packageVulnerabilityAudit) : undefined,
    handleResolutionPolicyViolations: policyHandlers?.handleResolutionPolicyViolations,
    runPacquet,
    ...(shouldPipeOwnLifecycleHooks(opts.loglevel, reporterName) ? { ownLifecycleHooksStdio: 'pipe' } : {}),
  }
  return {
    ...target,
    coreOpts,
    installOpts,
    manifest,
    writeProjectManifest,
    policyHandlers,
    workspacePackages,
    userNamedDeps,
  }
}

async function readManifestToInstall (
  opts: InstallDepsOptions,
  userNamedDeps: boolean
): Promise<{ manifest: ProjectManifest, writeProjectManifest: WriteProjectManifest }> {
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.dir, opts)
  if (manifest === null) {
    if (opts.update === true || !userNamedDeps) {
      throw new PnpmError('NO_PKG_MANIFEST', `No package.json found in ${opts.dir}`)
    }
    return { manifest: {}, writeProjectManifest }
  }
  if (opts.runtimeOnFail) {
    applyRuntimeOnFailOverride(manifest, opts.runtimeOnFail)
  }
  return { manifest, writeProjectManifest }
}

function shouldPipeOwnLifecycleHooks (loglevel: InstallDepsOptions['loglevel'], reporterName: InstallDepsOptions['reporter']): boolean {
  const isQuiet = loglevel === 'warn' || loglevel === 'error'
  return isQuiet && (reporterName == null || reporterName === 'default' || reporterName === 'append-only')
}

function selectUpdateTargets (ctx: SingleProjectInstallContext, params: string[]): DependencySelection | undefined {
  const { opts } = ctx
  if (opts.packageVulnerabilityAudit != null) {
    return {
      params,
      updatePackageManifest: opts.updatePackageManifest,
      updateMatching: createVulnerabilityUpdateMatching(opts.packageVulnerabilityAudit),
    }
  }
  const updateSpecs = selectUpdateSpecs(opts, params)
  const updateMatch = opts.update && updateSpecs.length ? createMatcher(updateSpecs) : null
  if (updateMatch == null) {
    return { params: updateSpecs, updatePackageManifest: opts.updatePackageManifest }
  }
  return matchDirectUpdateTargets(ctx, updateSpecs, updateMatch)
}

function selectUpdateSpecs (opts: InstallDepsOptions, params: string[]): string[] {
  if (!opts.update || params.length > 0) return params
  const ignoreDeps = opts.updateConfig?.ignoreDependencies
  return ignoreDeps?.length ? makeIgnorePatterns(ignoreDeps) : params
}

function matchDirectUpdateTargets (
  { opts, manifest, includeDirect }: SingleProjectInstallContext,
  updateSpecs: string[],
  updateMatch: UpdateDepsMatcher
): DependencySelection | undefined {
  const params = matchDependencies(updateMatch, manifest, includeDirect)
  let selection: DependencySelection = { params, updatePackageManifest: opts.updatePackageManifest }
  if (params.length === 0) {
    if (opts.latest) return undefined
    if (opts.depth === 0) {
      throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
        'None of the specified packages were found in the dependencies.')
    }
    // No direct dependencies matched, so we're updating indirect dependencies only
    // Don't update package.json in this case, and limit updates to only matching dependencies
    selection = {
      params,
      updatePackageManifest: false,
      updateMatching: (pkgName: string) => updateMatch(pkgName) != null,
    }
  }
  // At `--depth 0` an indirect dependency is never traversed, so a selector
  // that names one is simply out of scope rather than a version pnpm has
  // nowhere to record. `--latest` rejects every versioned selector on its
  // own, direct or not, and has to report that first.
  if (!opts.latest && (opts.depth ?? Infinity) > 0) {
    failOnVersionsOfIndirectUpdateSpecs(updateSpecs, [manifest], includeDirect)
  }
  return selection
}

function completeDependencySelectors (ctx: SingleProjectInstallContext, params: string[]): string[] {
  const { opts, manifest, includeDirect } = ctx
  const selectors = opts.update && opts.latest && params.length === 0
    ? Object.keys(filterDependenciesByType(manifest, includeDirect))
    : params
  if (!opts.workspace) return selectors
  return toWorkspaceSpecs(selectors, {
    manifest,
    include: includeDirect,
    workspacePackages: ctx.workspacePackages,
    userNamedDeps: ctx.userNamedDeps,
    fromInteractiveUpdate: opts.interactiveUpdate,
  })
}

async function installSomeDependencies (
  ctx: SingleProjectInstallContext,
  dependencySelectors: string[]
): Promise<DryRunInstallResult | undefined> {
  const { opts, allProjects } = ctx
  const saveManifests = createSaveManifestsOnce(ctx)
  const { updatedCatalogs, updatedProject, ignoredBuilds, newLockfile, wantedLockfile, resolutionPolicyViolations, dryRunResult } = await mutateModulesInSingleProject(createMutatedProject(ctx, dependencySelectors), {
    ...ctx.installOpts,
    beforeLifecycleScripts: async (res) => saveManifests({
      updatedProject: res.updatedProjects[0],
      updatedCatalogs: res.updatedCatalogs,
      newLockfile: res.newLockfile,
      wantedLockfile: res.wantedLockfile,
      resolutionPolicyViolations: res.resolutionPolicyViolations,
    }),
  })
  await saveManifests({
    updatedProject,
    updatedCatalogs,
    newLockfile,
    wantedLockfile,
    resolutionPolicyViolations,
  })
  await updateSingleProjectWorkspaceState({ opts, allProjects, manifest: updatedProject.manifest, updatedCatalogs })
  await handleIgnoredBuilds(opts, ignoredBuilds)
  return dryRunResult
}

function createMutatedProject ({ opts, manifest }: SingleProjectInstallContext, dependencySelectors: string[]) {
  return {
    allowNew: opts.allowNew,
    binsDir: opts.bin,
    dependencySelectors,
    manifest,
    mutation: 'installSome' as const,
    peer: opts.savePeer,
    peerAliases: opts.peer === true
      ? new Set(dependencySelectors
        .map((selector) => parseWantedDependency(selector).alias)
        .filter((alias): alias is string => alias != null && Object.hasOwn(manifest.peerDependencies ?? {}, alias)))
      : undefined,
    rangeSpecStyle: getRangeSpecStyle(opts),
    rootDir: opts.dir as ProjectRootDir,
    targetDependenciesField: getSaveType(opts),
  }
}

interface ManifestsToSave {
  updatedProject?: { manifest: ProjectManifest }
  updatedCatalogs?: Catalogs
  newLockfile?: LockfileObject
  wantedLockfile?: LockfileObject
  resolutionPolicyViolations?: ResolutionPolicyViolation[]
}

function createSaveManifestsOnce (
  { opts, policyHandlers, writeProjectManifest }: SingleProjectInstallContext
): (manifests: ManifestsToSave) => Promise<void> {
  let manifestsSaved = false
  return async ({ updatedProject, updatedCatalogs, newLockfile, wantedLockfile, resolutionPolicyViolations }) => {
    if (manifestsSaved) return
    manifestsSaved = true
    if (opts.save === false || opts.dryRun || !updatedProject) return
    // Only pick entries when we'll actually persist. Otherwise the
    // info log would claim we added entries the workspace manifest
    // never saw, and the next install would re-prompt or fail
    // verification.
    const policyUpdates = policyHandlers?.pickManifestUpdates(resolutionPolicyViolations ?? [])
    await Promise.all([
      writeProjectManifest(updatedProject.manifest),
      updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
        updatedCatalogs,
        catalogPrune: opts.catalogPrune,
        keptCatalogs: keptCatalogsForPrune(opts, newLockfile, wantedLockfile),
        resolvedPackageVersions: resolvedPackageVersionsForPrune(opts, newLockfile),
        minimumReleaseAgeExcludePrune: opts.minimumReleaseAgeExcludePrune,
        trustPolicyExcludePrune: opts.trustPolicyExcludePrune,
        allProjects: opts.allProjects,
        ...policyUpdates,
      }),
    ])
  }
}

async function installAllDependencies (
  ctx: SingleProjectInstallContext,
  selection: DependencySelection
): Promise<DryRunInstallResult | undefined> {
  const { opts, allProjects } = ctx
  const installResult = await install(ctx.manifest, {
    ...ctx.installOpts,
    updatePackageManifest: selection.updatePackageManifest,
    updateMatching: selection.updateMatching,
  })
  await persistInstallManifests(ctx, installResult)
  await handleIgnoredBuilds(opts, installResult.ignoredBuilds)

  if (opts.linkWorkspacePackages && opts.workspaceDir) {
    await installLinkedWorkspaceProjects(ctx, { workspaceDir: opts.workspaceDir, updatedCatalogs: installResult.updatedCatalogs })
    if (opts.ignoreScripts) return undefined
    await buildInstalledProject(ctx)
  } else {
    await updateSingleProjectWorkspaceState({
      opts,
      allProjects,
      manifest: installResult.updatedManifest,
      updatedCatalogs: installResult.updatedCatalogs,
    })
  }
  return installResult.dryRunResult
}

type InstallResult = Awaited<ReturnType<typeof install>>

async function persistInstallManifests (
  { opts, allProjects, policyHandlers, writeProjectManifest }: SingleProjectInstallContext,
  { updatedCatalogs, updatedManifest, newLockfile, resolutionPolicyViolations }: InstallResult
): Promise<void> {
  // `opts.save === false` (e.g. `--no-save`) means "don't persist anything
  // from this install" — both package.json and the workspace manifest.
  // Skip the pick so the info log doesn't claim entries were added that
  // were never written; the next install will resurface them.
  if (opts.save === false || opts.dryRun) return
  const policyUpdates = policyHandlers?.pickManifestUpdates(resolutionPolicyViolations)
  if (opts.update === true) {
    await Promise.all([
      writeProjectManifest(updatedManifest),
      updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, {
        updatedCatalogs,
        catalogPrune: opts.catalogPrune,
        keptCatalogs: keptCatalogsForPrune(opts, newLockfile),
        resolvedPackageVersions: resolvedPackageVersionsForPrune(opts, newLockfile),
        minimumReleaseAgeExcludePrune: opts.minimumReleaseAgeExcludePrune,
        trustPolicyExcludePrune: opts.trustPolicyExcludePrune,
        allProjects,
        ...policyUpdates,
      }),
    ])
  } else if (policyUpdates != null) {
    // Plain `pnpm install` (no --update, no params) wouldn't otherwise touch
    // the workspace manifest. Persist the auto-policy patches anyway so any
    // loose bypass (today: minimumReleaseAgeExclude) remains explicit on
    // subsequent installs.
    await updateWorkspaceManifest(opts.workspaceDir ?? opts.dir, policyUpdates)
  }
}

async function installLinkedWorkspaceProjects (
  { opts, allProjects, runPacquet }: SingleProjectInstallContext,
  { workspaceDir, updatedCatalogs }: { workspaceDir: string, updatedCatalogs?: Catalogs }
): Promise<void> {
  const { selectedProjectsGraph } = await filterProjectsBySelectorObjects(allProjects, [
    {
      excludeSelf: true,
      includeDependencies: true,
      parentDir: opts.dir || process.cwd(),
    },
  ], {
    catalogs: opts.catalogs,
    workspaceDir,
  })
  await recursiveInstallThenUpdateWorkspaceState(allProjects, [], {
    ...opts,
    ...OVERWRITE_UPDATE_OPTIONS,
    allProjectsGraph: opts.allProjectsGraph!,
    selectedProjectsGraph,
    workspaceDir,
    runPacquet,
  }, 'install', updatedCatalogs)
}

async function buildInstalledProject ({ opts, coreOpts, store, userNamedDeps }: SingleProjectInstallContext): Promise<void> {
  await buildProjects(
    [
      {
        buildIndex: 0,
        manifest: await readProjectManifestOnly(opts.dir, opts),
        rootDir: opts.dir as ProjectRootDir,
      },
    ], {
      ...coreOpts,
      pending: true,
      storeController: store.ctrl,
      storeDir: store.dir,
      skipIfHasSideEffectsCache: true,
      ...(userNamedDeps ? { stages: PROJECT_INSTALL_STAGES } : {}),
    }
  )
}
