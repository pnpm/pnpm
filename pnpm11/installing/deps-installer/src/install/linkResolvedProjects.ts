import path from 'node:path'

import { buildModules, type DepsStateCache } from '@pnpm/building.during-install'
import { isBuildExplicitlyDisallowed } from '@pnpm/building.policy'
import { installabilityUnderForce } from '@pnpm/config.package-is-installable'
import { LAYOUT_VERSION } from '@pnpm/constants'
import {
  makeNodePackageMapOption,
  makeNodeRequireOption,
  POST_UNINSTALL_STAGES,
  PROJECT_INSTALL_STAGES,
  PROJECT_LIFECYCLE_STAGES,
  runLifecycleHooksConcurrently,
} from '@pnpm/exec.lifecycle'
import type { PnpmContext } from '@pnpm/installing.context'
import { extendProjectsWithTargetDirs, getInjectedDeps, type InstallationResultStats } from '@pnpm/installing.deps-restorer'
import { writeModulesManifest } from '@pnpm/installing.modules-yaml'
import { writeCurrentLockfile } from '@pnpm/lockfile.fs'
import { PACKAGE_MAP_FILENAME, removePackageMap, writePackageMap, writePnpFile } from '@pnpm/lockfile.to-pnp'
import { findLockedRootNodeRuntime } from '@pnpm/lockfile.utils'
import type { DepPath, IgnoredBuilds, PeerDependencyIssuesByProjects } from '@pnpm/types'
import { pathAbsolute } from 'path-absolute'

import { linkPackages } from './link.js'
import { linkBinsOfResolvedProjects, linkRuntimeBinsOfImporters } from './linkBins.js'
import type { ImporterToUpdate, InstallInContextOptions, ProjectToBeInstalled, UpdatedProject } from './mutationTypes.js'
import type { GraphResolution } from './resolveProjects.js'
import { writeLockfilesAndRecordVerified } from './writeLockfilesAndRecordVerified.js'

/** The state the steps that materialize a resolved graph share. */
export interface LinkContext {
  ctx: PnpmContext
  opts: InstallInContextOptions
  projects: ImporterToUpdate[]
  resolution: GraphResolution
  depsStateCache: DepsStateCache
  /**
   * Nothing reads `.package-map.json` unless `nodeExperimentalPackageMap`
   * is on: `pnpm run` / `pnpm exec` only pass it to Node under that
   * setting.
   */
  shouldWritePackageMap: boolean
}

type LinkPackagesResult = Awaited<ReturnType<typeof linkPackages>>

export async function linkResolvedProjects (
  step: LinkContext
): Promise<{ stats: InstallationResultStats, ignoredBuilds: IgnoredBuilds | undefined }> {
  const { ctx, opts, projects, resolution } = step
  const result = await linkPackages(projects, resolution.dependenciesGraph, {
    ...describeLinkedGraph(step),
    ...describeLinkingPolicies(step),
  })
  await writeResolutionFiles(step, result)

  ctx.pendingBuilds = ctx.pendingBuilds
    .filter((relDepPath) => !result.removedDepPaths.has(relDepPath))

  let ignoredBuilds = collectPreviouslyIgnoredBuilds(step, result)
  if (result.newDepPaths?.length) {
    ignoredBuilds = await buildNewDependencies(step, { ignoredBuilds, newDepPaths: result.newDepPaths })
  }

  await linkBinsOfResolvedProjects(step, result.newDepPaths)

  const injectionTargetsByDepPath = getInjectionTargetsByDepPath(resolution)
  const projectsWithTargetDirs = extendProjectsWithTargetDirs(projects, injectionTargetsByDepPath, opts.lockfileDir)
  await Promise.all([
    writeLockfiles(step, result),
    writeModulesManifestIfChanged(step, { ignoredBuilds, injectionTargetsByDepPath, result }),
  ])
  await opts.verifyLockfile?.()
  await opts.beforeLifecycleScripts?.({
    updatedProjects: toUpdatedProjects(projects, resolution.peerDependencyIssuesByProjects),
    updatedCatalogs: resolution.updatedCatalogs,
    newLockfile: resolution.newLockfile,
    resolutionPolicyViolations: resolution.resolutionPolicyViolations,
  })
  if (!opts.ignoreScripts && !opts.virtualStoreOnly) {
    await runProjectLifecycleScripts(step, projectsWithTargetDirs)
  }
  return { stats: result.stats, ignoredBuilds }
}

export function toUpdatedProjects (
  projects: ImporterToUpdate[],
  peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
): UpdatedProject[] {
  return projects.map(({ id, manifest, originalManifest, rootDir }) => ({
    originalManifest,
    manifest,
    peerDependencyIssues: peerDependencyIssuesByProjects[id],
    rootDir,
  }))
}

function describeLinkedGraph ({ ctx, depsStateCache, opts, resolution }: LinkContext) {
  return {
    currentLockfile: ctx.currentLockfile,
    dependenciesByProjectId: resolution.dependenciesByProjectId,
    depsStateCache,
    extraNodePaths: ctx.extraNodePaths,
    hoistedDependencies: ctx.hoistedDependencies,
    hoistedModulesDir: ctx.hoistedModulesDir,
    hoistPattern: ctx.hoistPattern,
    linkedDependenciesByProjectId: resolution.linkedDependenciesByProjectId,
    lockfileDir: opts.lockfileDir,
    outdatedDependencies: resolution.outdatedDependencies,
    publicHoistPattern: ctx.publicHoistPattern,
    registriesByScope: ctx.registriesByScope,
    rootModulesDir: ctx.rootModulesDir,
    skipped: ctx.skipped,
    storeController: opts.storeController,
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
    wantedLockfile: resolution.newLockfile,
    wantedToBeSkippedPackageIds: resolution.wantedToBeSkippedPackageIds,
  }
}

function describeLinkingPolicies ({ opts }: LinkContext) {
  return {
    allowBuild: opts.allowBuild,
    dedupeDirectDeps: opts.dedupeDirectDeps,
    deferDependencyBuilds: opts.deferDependencyBuilds,
    disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
    enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
    force: opts.force,
    ignoreScripts: opts.ignoreScripts,
    include: opts.include,
    makePartialCurrentLockfile: opts.makePartialCurrentLockfile,
    pruneStore: opts.pruneStore,
    pruneVirtualStore: opts.pruneVirtualStore,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    sideEffectsCacheRead: opts.sideEffectsCacheRead,
    remoteSideEffectsCache: opts.remoteSideEffectsCache,
    pnprServer: opts.pnprServer,
    configByUri: opts.configByUri,
    symlink: opts.symlink,
    skipRuntimes: opts.skipRuntimes,
    hoistWorkspacePackages: opts.hoistWorkspacePackages,
    virtualStoreOnly: opts.virtualStoreOnly,
    supportedArchitectures: opts.supportedArchitectures,
  }
}

async function writeResolutionFiles (step: LinkContext, result: LinkPackagesResult): Promise<void> {
  const { ctx, opts, projects, resolution } = step
  if (step.shouldWritePackageMap) {
    // Omit the importer self-mapping when a project has no name (see the
    // deps-restorer write): a non-package-name key like `.` would be invalid.
    const importerNames = Object.fromEntries(
      projects.map(({ manifest, id }) => [id, manifest.name])
    )
    await writePackageMap(result.currentLockfile, {
      importerNames,
      lockfileDir: ctx.lockfileDir,
      locationByDepPath: Object.fromEntries(
        Object.values(resolution.dependenciesGraph).map((node) => [node.depPath, node.dir])
      ),
      packageMapType: opts.nodePackageMapType,
      rootModulesDir: ctx.rootModulesDir,
      virtualStoreDir: ctx.virtualStoreDir,
      virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
    })
  } else if (!opts.virtualStoreOnly) {
    // An install that stops writing the map must not leave the previous
    // one behind for the next `pnpm run` to hand to Node.
    await removePackageMap(ctx.rootModulesDir)
  }
  // `.pnp.cjs` is how a PnP project resolves, which makes it a
  // project-level artifact like the importer symlinks and the package
  // map. `virtualStoreOnly` — how `pnpm fetch` warms a store without
  // touching the project — must not write it.
  if (opts.enablePnp && !opts.virtualStoreOnly) {
    const importerNames = Object.fromEntries(
      projects.map(({ manifest, id }) => [id, manifest.name ?? id])
    )
    await writePnpFile(result.currentLockfile, {
      importerNames,
      lockfileDir: ctx.lockfileDir,
      virtualStoreDir: ctx.virtualStoreDir,
      virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
      registriesByScope: ctx.registriesByScope,
    })
  }
}

function collectPreviouslyIgnoredBuilds ({ ctx, opts }: LinkContext, result: LinkPackagesResult): IgnoredBuilds | undefined {
  const lockfilePackages = result.currentLockfile.packages
  if (!ctx.modulesFile?.ignoredBuilds?.size || lockfilePackages == null) return undefined
  let ignoredBuilds: IgnoredBuilds | undefined
  for (const ignoredBuild of ctx.modulesFile.ignoredBuilds.values()) {
    // `Object.hasOwn` keeps a `.modules.yaml` entry named like an
    // `Object.prototype` key from matching a package the lockfile
    // doesn't contain.
    if (Object.hasOwn(lockfilePackages, ignoredBuild) && !isBuildExplicitlyDisallowed(ignoredBuild, opts.allowBuild)) {
      ignoredBuilds ??= new Set()
      ignoredBuilds.add(ignoredBuild)
    }
  }
  return ignoredBuilds
}

async function buildNewDependencies (
  step: LinkContext,
  { ignoredBuilds, newDepPaths }: { ignoredBuilds: IgnoredBuilds | undefined, newDepPaths: DepPath[] }
): Promise<IgnoredBuilds | undefined> {
  const { ctx, opts, resolution } = step
  if (opts.ignoreScripts) {
    // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
    ctx.pendingBuilds = ctx.pendingBuilds
      .concat(
        newDepPaths.filter((depPath) => resolution.dependenciesGraph[depPath].requiresBuild)
      )
  }
  if (opts.ignoreScripts && Object.keys(opts.patchedDependencies ?? {}).length === 0) return ignoredBuilds
  // postinstall hooks
  const ignoredBuildsFromBuild = await buildDependencies(step, newDepPaths)
  if (!ignoredBuildsFromBuild?.size) return ignoredBuilds
  const allIgnoredBuilds = ignoredBuilds ?? new Set()
  for (const ignoredBuild of ignoredBuildsFromBuild.values()) {
    allIgnoredBuilds.add(ignoredBuild)
  }
  return allIgnoredBuilds
}

async function buildDependencies (step: LinkContext, newDepPaths: DepPath[]): Promise<IgnoredBuilds | undefined> {
  const { ctx, opts, projects, resolution } = step
  const { dependenciesGraph } = resolution
  const depPaths = Object.keys(dependenciesGraph) as DepPath[]
  const rootNodes = depPaths.filter((depPath) => dependenciesGraph[depPath].depth === 0)

  // Only point Node at the loader when it was actually written —
  // `--require` on a missing file fails the script before it runs.
  const extraEnv = addNodeLoaderEnv(step, opts.scriptsOpts.extraEnv)
  if (!opts.ignoreScripts && !opts.virtualStoreOnly) {
    await linkRuntimeBinsOfImporters({
      dependenciesByProjectId: resolution.dependenciesByProjectId,
      dependenciesGraph,
      extraNodePaths: ctx.extraNodePaths,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      projects,
    })
  }
  // Dependency lifecycle scripts must not run on an unverified lockfile.
  await opts.verifyLockfile?.()
  return (await buildModules(dependenciesGraph, rootNodes, {
    ...describeBuildEnvironment(step, extraEnv),
    depsToBuild: new Set(newDepPaths),
  })).ignoredBuilds
}

function describeBuildEnvironment ({ ctx, depsStateCache, opts, resolution }: LinkContext, extraEnv: Record<string, string> | undefined) {
  return {
    engineStrict: installabilityUnderForce(opts).engineStrict,
    engineNodeVersion: opts.nodeVersion,
    linkedModulesDirs: [
      ...opts.allProjects.map((project) => pathAbsolute(project.modulesDir ?? opts.modulesDir ?? 'node_modules', project.rootDir)),
      ctx.hoistedModulesDir,
    ],
    skipped: ctx.skipped,
    allowBuild: opts.allowBuild,
    childConcurrency: opts.childConcurrency,
    depsStateCache,
    extraBinPaths: ctx.extraBinPaths,
    extraNodePaths: ctx.extraNodePaths,
    extraEnv,
    ignoreScripts: opts.ignoreScripts,
    lockfileDir: ctx.lockfileDir,
    nodeVersion: findLockedRootNodeRuntime(resolution.newLockfile)?.version,
    optional: opts.include.optionalDependencies,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    rootModulesDir: ctx.virtualStoreDir,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
    remoteSideEffectsCache: opts.remoteSideEffectsCache,
    storeController: opts.storeController,
    supportedArchitectures: opts.supportedArchitectures,
    unsafePerm: opts.unsafePerm,
    userAgent: opts.userAgent,
    enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
    frozenStore: opts.frozenStore,
    configByUri: opts.configByUri,
    pnprServer: opts.pnprServer,
  }
}

/**
 * Adds the options that point Node at the PnP loader and the package map
 * this install wrote.
 */
function addNodeLoaderEnv (
  { ctx, opts, shouldWritePackageMap }: LinkContext,
  baseEnv: Record<string, string> | undefined
): Record<string, string> | undefined {
  let extraEnv = baseEnv
  if (opts.enablePnp && !opts.virtualStoreOnly) {
    extraEnv = {
      ...extraEnv,
      ...makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs'), extraEnv),
    }
  }
  if (opts.nodeExperimentalPackageMap && shouldWritePackageMap) {
    extraEnv = {
      ...extraEnv,
      ...makeNodePackageMapOption(path.join(ctx.rootModulesDir, PACKAGE_MAP_FILENAME), extraEnv),
    }
  }
  return extraEnv
}

/**
 * Build injectionTargetsByDepPath from the dependenciesGraph for injected workspace packages.
 * The dependenciesGraph already has the correct `dir` values after `extendGraph` is applied
 * (which uses the correct hash-based paths when global virtual store is enabled).
 */
function getInjectionTargetsByDepPath ({ dependenciesGraph, newLockfile }: GraphResolution): Map<string, string[]> {
  const injectionTargetsByDepPath = new Map<string, string[]>()
  for (const [depPath, { resolution }] of Object.entries(newLockfile.packages ?? {})) {
    if (resolution?.type !== 'directory') continue
    const graphNode = dependenciesGraph[depPath as DepPath]
    if (graphNode?.dir) {
      injectionTargetsByDepPath.set(depPath, [graphNode.dir])
    }
  }
  return injectionTargetsByDepPath
}

async function writeLockfiles ({ ctx, opts, resolution }: LinkContext, result: LinkPackagesResult): Promise<void> {
  if (opts.useLockfile && opts.saveLockfile) {
    await writeLockfilesAndRecordVerified({
      currentLockfile: result.currentLockfile,
      currentLockfileDir: path.join(ctx.rootModulesDir, '.pnpm'),
      wantedLockfile: resolution.newLockfile,
      wantedLockfileDir: ctx.lockfileDir,
      cacheDir: opts.cacheDir,
      resolutionVerifiers: opts.resolutionVerifiers,
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
  } else {
    await writeCurrentLockfile(ctx.virtualStoreDir, result.currentLockfile)
  }
}

async function writeModulesManifestIfChanged (
  { ctx, opts }: LinkContext,
  { ignoredBuilds, injectionTargetsByDepPath, result }: {
    ignoredBuilds: IgnoredBuilds | undefined
    injectionTargetsByDepPath: Map<string, string[]>
    result: LinkPackagesResult
  }
): Promise<void> {
  if (
    result.currentLockfile.packages === undefined &&
    result.removedDepPaths.size === 0 &&
    Object.keys(ctx.hoistedDependencies).length === 0 &&
    Object.keys(result.newHoistedDependencies).length === 0
  ) {
    return
  }
  const injectedDeps = getInjectedDeps(injectionTargetsByDepPath, opts.lockfileDir)
  await writeModulesManifest(ctx.rootModulesDir, {
    ...ctx.modulesFile,
    hoistedDependencies: result.newHoistedDependencies,
    hoistPattern: ctx.hoistPattern,
    included: ctx.include,
    injectedDeps,
    ignoredBuilds,
    layoutVersion: LAYOUT_VERSION,
    nodeLinker: opts.nodeLinker,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    publicHoistPattern: ctx.publicHoistPattern,
    virtualStoreOnly: opts.virtualStoreOnly,
    prunedAt: opts.pruneVirtualStore || ctx.modulesFile == null
      ? new Date().toUTCString()
      : ctx.modulesFile.prunedAt,
    skipped: Array.from(ctx.skipped),
    storeDir: ctx.storeDir,
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
    allowBuilds: opts.allowBuilds,
  })
}

async function runProjectLifecycleScripts (
  step: LinkContext,
  projectsWithTargetDirs: ReturnType<typeof extendProjectsWithTargetDirs<ImporterToUpdate>>
): Promise<void> {
  const { opts } = step
  opts.scriptsOpts.extraEnv = addNodeLoaderEnv(step, opts.scriptsOpts.extraEnv)
  const projectsToBeBuilt = projectsWithTargetDirs
    .filter(({ mutation, rootDir }) => mutation === 'install' || (mutation === 'uninstallSome' && opts.projectDirsRemovingDeps.has(rootDir)))
    .map((project) => project.mutation === 'uninstallSome' ? { ...project, stages: POST_UNINSTALL_STAGES } : project) as ProjectToBeInstalled[]
  // The projects' own lifecycle scripts import dependency code linked
  // from the lockfile, so they are held to the same gate as dependency
  // builds — also when no new dep paths made the buildModules branch run.
  await opts.verifyLockfile?.()
  await runLifecycleHooksConcurrently({
    childConcurrency: opts.childConcurrency,
    importers: projectsToBeBuilt,
    opts: opts.scriptsOpts,
    projectDependencies: opts.projectDependencies,
    projectWithPreinstallRan: opts.rootProjectPreinstallRan ? opts.lockfileDir : undefined,
    stages: (opts.deploy || opts.include?.devDependencies === false)
      ? PROJECT_INSTALL_STAGES
      : PROJECT_LIFECYCLE_STAGES,
  })
}
