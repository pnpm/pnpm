import path from 'node:path'

import type { ProjectConfig } from '@pnpm/config.reader'
import {
  type BeforeLifecycleScriptsResult,
  type MutatedProject,
  mutateModules,
} from '@pnpm/installing.deps-installer'
import type { ProjectRootDir } from '@pnpm/types'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { isSubdir } from 'is-subdir'

import { handleIgnoredBuilds } from '../handleIgnoredBuilds.js'
import { keptCatalogsForPrune, resolvedPackageVersionsForPrune } from '../resolvedPackageVersionsForPrune.js'
import {
  completeDependencySelectors,
  createNoPackageInDependenciesError,
  getProjectRangeSpecStyle,
  type RecursiveContext,
  selectUpdateTargets,
} from './context.js'
import type { RecursiveOptions, RecursiveResult } from './options.js'

type Mutation = 'install' | 'installSome' | 'uninstallSome'

interface ImporterToMutate {
  mutation: Mutation
  rootDir: ProjectRootDir
}

/**
 * Installs the selected projects of a workspace with a shared lockfile in a
 * single `mutateModules` call.
 */
export async function installWithSharedLockfile (ctx: RecursiveContext): Promise<RecursiveResult> {
  const importers = getImportersInsideRepository(ctx.opts)
  if (importers.length === 0) return { passed: true }
  const mutatedImporters = await createMutatedImporters(ctx, importers)
  if ((mutatedImporters.length === 0) && ctx.cmdFullName === 'update' && ctx.opts.depth === 0) {
    throw createNoPackageInDependenciesError()
  }
  if (shouldInstallWorkspaceRoot(ctx)) {
    mutatedImporters.push({
      mutation: 'install',
      rootDir: ctx.opts.workspaceDir as ProjectRootDir,
    })
  }
  const saveManifests = createManifestsSaver(ctx)
  const {
    updatedCatalogs,
    updatedProjects,
    ignoredBuilds,
    newLockfile,
    wantedLockfile,
    resolutionPolicyViolations,
    dryRunResult,
  } = await mutateModules(mutatedImporters, {
    ...ctx.installOpts,
    storeController: ctx.store.ctrl,
    resolutionVerifiers: ctx.store.resolutionVerifiers,
    beforeLifecycleScripts: saveManifests,
  })
  await saveManifests({
    updatedProjects,
    updatedCatalogs,
    newLockfile,
    wantedLockfile,
    resolutionPolicyViolations,
  })
  await handleIgnoredBuilds(ctx.opts, ignoredBuilds)
  return { passed: true, updatedCatalogs, dryRunResult }
}

function getImportersInsideRepository (
  opts: Pick<RecursiveOptions, 'selectedProjectsGraph' | 'ignoredPackages' | 'workspaceDir'>
): Array<{ rootDir: ProjectRootDir }> {
  const importers = getImporters(opts)
  const calculatedRepositoryRoot = calculateRepositoryRoot(opts.workspaceDir, importers.map(({ rootDir }) => rootDir))
  const isFromWorkspace = isSubdir.bind(null, calculatedRepositoryRoot)
  return importers.filter(({ rootDir }) => isFromWorkspace(rootDir))
}

function getImporters (opts: Pick<RecursiveOptions, 'selectedProjectsGraph' | 'ignoredPackages'>): Array<{ rootDir: ProjectRootDir }> {
  let rootDirs = Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]
  if (opts.ignoredPackages != null) {
    rootDirs = rootDirs.filter((rootDir) => !opts.ignoredPackages!.has(rootDir))
  }
  return rootDirs.map((rootDir) => ({ rootDir }))
}

function calculateRepositoryRoot (
  workspaceDir: string,
  projectDirs: string[]
): string {
  // assume repo root is workspace dir
  let relativeRepoRoot = '.'
  for (const rootDir of projectDirs) {
    const relativePartRegExp = new RegExp(`^(\\.\\.\\${path.sep})+`)
    const relativePartMatch = relativePartRegExp.exec(path.relative(workspaceDir, rootDir))
    if (relativePartMatch == null) continue
    const relativePart = relativePartMatch[0]
    if (relativePart.length > relativeRepoRoot.length) {
      relativeRepoRoot = relativePart
    }
  }
  return path.resolve(workspaceDir, relativeRepoRoot)
}

async function createMutatedImporters (
  ctx: RecursiveContext,
  importers: Array<{ rootDir: ProjectRootDir }>
): Promise<MutatedProject[]> {
  const mutation = selectMutation(ctx)
  const mutatedImporters = [] as MutatedProject[]
  await Promise.all(importers.map(async ({ rootDir }) => {
    const mutatedImporter = createMutatedImporter(ctx, { mutation, rootDir })
    if (mutatedImporter != null) {
      mutatedImporters.push(mutatedImporter)
    }
  }))
  return mutatedImporters
}

function selectMutation ({ cmdFullName, params, updateToLatest }: RecursiveContext): Mutation {
  switch (cmdFullName) {
    case 'remove':
      return 'uninstallSome'
    case 'import':
      return 'install'
    default:
      return (params.length === 0 && !updateToLatest ? 'install' : 'installSome')
  }
}

/**
 * The mutation of one selected project, or `undefined` for a project an
 * update skips because none of its dependencies match the update selectors.
 */
function createMutatedImporter (
  ctx: RecursiveContext,
  { mutation, rootDir }: ImporterToMutate
): MutatedProject | undefined {
  const { manifest } = ctx.manifestsByPath[rootDir]
  const localConfig = ctx.getProjectConfig(manifest) ?? {}
  const updateTargets = selectUpdateTargets(ctx, manifest)
  if (ctx.updateMatch != null && (updateTargets.length === 0) && (typeof ctx.opts.depth === 'undefined' || ctx.opts.depth <= 0)) {
    ctx.installOpts.pruneLockfileImporters = false
    return undefined
  }
  const dependencySelectors = completeDependencySelectors(ctx, manifest, updateTargets)
  const modulesDir = localConfig.modulesDir ?? ctx.opts.modulesDir
  switch (mutation) {
    case 'uninstallSome':
      return {
        dependencyNames: dependencySelectors,
        modulesDir,
        mutation,
        rootDir,
        targetDependenciesField: ctx.targetDependenciesField,
      } as MutatedProject
    case 'installSome':
      return createInstallSomeMutation(ctx, { dependencySelectors, localConfig, modulesDir, rootDir })
    case 'install':
      return {
        modulesDir,
        mutation,
        pruneDirectDependencies: ctx.opts.pruneDirectDependencies,
        rootDir,
        update: ctx.opts.update,
        updateMatching: ctx.opts.updateMatching,
        updatePackageManifest: ctx.opts.updatePackageManifest,
        updateToLatest: ctx.opts.latest,
      } as MutatedProject
  }
}

interface InstallSomeImporter {
  dependencySelectors: string[]
  localConfig: ProjectConfig
  modulesDir: string | undefined
  rootDir: ProjectRootDir
}

function createInstallSomeMutation (
  { cmdFullName, opts, targetDependenciesField }: RecursiveContext,
  { dependencySelectors, localConfig, modulesDir, rootDir }: InstallSomeImporter
): MutatedProject {
  return {
    allowNew: cmdFullName === 'install' || cmdFullName === 'add',
    dependencySelectors,
    modulesDir,
    mutation: 'installSome',
    peer: opts.savePeer,
    rangeSpecStyle: getProjectRangeSpecStyle(opts, localConfig),
    rootDir,
    targetDependenciesField,
    update: opts.update,
    updateMatching: opts.updateMatching,
    updatePackageManifest: opts.updatePackageManifest,
    updateToLatest: opts.latest,
  } as MutatedProject
}

function shouldInstallWorkspaceRoot ({ opts, manifestsByPath }: RecursiveContext): boolean {
  return !opts.excludeWorkspaceRootProject &&
    !opts.selectedProjectsGraph[opts.workspaceDir as ProjectRootDir] &&
    manifestsByPath[opts.workspaceDir as ProjectRootDir] != null
}

/**
 * Saves the manifests before the lifecycle scripts run, or after the install
 * when the install ran no lifecycle scripts. Only the first call saves.
 */
function createManifestsSaver (ctx: RecursiveContext): (result: BeforeLifecycleScriptsResult) => Promise<void> {
  let manifestsSaved = false
  return async (result) => {
    if (manifestsSaved) return
    manifestsSaved = true
    if (ctx.opts.save !== false && !ctx.opts.dryRun) {
      await persistManifests(ctx, result)
    }
  }
}

async function persistManifests (
  { allProjects, manifestsByPath, opts, policyHandlers }: RecursiveContext,
  { updatedProjects, updatedCatalogs, newLockfile, wantedLockfile, resolutionPolicyViolations }: BeforeLifecycleScriptsResult
): Promise<void> {
  // Only pick entries when we'll actually persist. Otherwise the
  // info log would claim entries were added that the workspace
  // manifest never saw, and the next install would re-prompt or
  // fail verification.
  const policyUpdates = policyHandlers?.pickManifestUpdates(resolutionPolicyViolations ?? [])
  const promises: Array<Promise<void>> = updatedProjects
    .filter(({ rootDir }) => manifestsByPath[rootDir] != null)
    .map(async ({ originalManifest, manifest, rootDir }) => {
      return manifestsByPath[rootDir].writeProjectManifest(originalManifest ?? manifest)
    })
  promises.push(updateWorkspaceManifest(opts.workspaceDir, {
    updatedCatalogs,
    catalogPrune: opts.catalogPrune,
    keptCatalogs: keptCatalogsForPrune(opts, newLockfile, wantedLockfile),
    resolvedPackageVersions: resolvedPackageVersionsForPrune(opts, newLockfile),
    minimumReleaseAgeExcludePrune: opts.minimumReleaseAgeExcludePrune,
    trustPolicyExcludePrune: opts.trustPolicyExcludePrune,
    allProjects,
    ...policyUpdates,
  }))
  await Promise.all(promises)
}
