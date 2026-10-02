import type { DepsStateCache } from '@pnpm/building.during-install'
import { stageLogger, summaryLogger } from '@pnpm/core-loggers'
import { createOverriddenDependencyMatcher } from '@pnpm/hooks.read-package-hook'
import type { PnpmContext } from '@pnpm/installing.context'
import type { InstallationResultStats } from '@pnpm/installing.deps-restorer'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, IgnoredBuilds, ProjectId } from '@pnpm/types'
import { clone } from 'ramda'

import { isCheckOnlyInstall } from './installPredicates.js'
import { linkResolvedProjects, toUpdatedProjects } from './linkResolvedProjects.js'
import type { ImporterToUpdate, InstallFunctionResult, InstallInContextOptions } from './mutationTypes.js'
import { removeDepsFromManifests } from './removeDepsFromManifests.js'
import { reportPeerDependencyIssues } from './reportPeerDependencyIssues.js'
import { type GraphResolution, resolveProjects } from './resolveProjects.js'
import { writeWantedLockfileAndRecordVerified } from './writeWantedLockfileAndRecordVerified.js'

/**
 * Resolves the projects' dependencies and, unless the install only writes
 * the lockfile, materializes the resolved graph.
 */
export async function resolveAndLinkProjects (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: InstallInContextOptions
): Promise<InstallFunctionResult> {
  // Aliasing for clarity in boolean expressions below.
  const isInstallationOnlyForLockfileCheck = isCheckOnlyInstall(opts)

  // The wanted lockfile is mutated during installation. To compare changes, a
  // deep copy before installation is needed. This copy should represent the
  // original wanted lockfile on disk as close as possible.
  //
  // This object can be quite large. Intentionally avoiding an expensive copy
  // unless this is a check-only install that needs the comparison.
  const originalLockfileForCheck = isInstallationOnlyForLockfileCheck
    ? clone(ctx.wantedLockfile)
    : null

  await prepareProjectsForResolution(projects, ctx, opts)
  const resolution = await resolveProjects(projects, ctx, opts)

  const depsStateCache: DepsStateCache = {}
  const shouldWritePackageMap = opts.nodeExperimentalPackageMap && opts.enableModulesDir !== false && opts.nodeLinker === 'isolated' && !opts.virtualStoreOnly
  let stats: InstallationResultStats | undefined
  let ignoredBuilds: IgnoredBuilds | undefined
  if (!opts.lockfileOnly && !isInstallationOnlyForLockfileCheck && opts.enableModulesDir) {
    ({ stats, ignoredBuilds } = await linkResolvedProjects({
      ctx,
      depsStateCache,
      opts,
      projects,
      resolution,
      shouldWritePackageMap,
    }))
  } else {
    await writeLockfileWithoutLinking(ctx, { isInstallationOnlyForLockfileCheck, newLockfile: resolution.newLockfile, opts })
  }

  await resolution.waitTillAllFetchingsFinish()
  const depsRequiringBuild = await listDepsRequiringBuild(resolution, opts)
  reportResolutionOutcome(resolution, { opts, originalLockfileForCheck })

  return {
    updatedCatalogs: resolution.updatedCatalogs,
    newLockfile: resolution.newLockfile,
    projects: toUpdatedProjects(projects, resolution.peerDependencyIssuesByProjects),
    stats,
    depsRequiringBuild,
    ignoredBuilds,
    resolutionPolicyViolations: resolution.resolutionPolicyViolations,
    dryRunResult: (opts.dryRun && originalLockfileForCheck != null)
      ? { originalLockfile: originalLockfileForCheck, wantedLockfile: resolution.newLockfile }
      : undefined,
  }
}

async function prepareProjectsForResolution (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: InstallInContextOptions
): Promise<void> {
  addMissingImporters(ctx, projects)
  if (opts.pruneLockfileImporters) {
    pruneUnselectedImporters(ctx, projects)
  }

  await Promise.all(
    projects
      .map(async (project) => {
        if (project.mutation !== 'uninstallSome') return
        await removeDepsFromManifests(project, project)
      })
  )

  // Only the projects whose manifest this run writes need the answer. A parent-scoped override
  // (`parent>child`) is selected by the name and version of the manifest it is matched against,
  // and `createReadPackageHook` runs the overrides hook after `packageExtensions` and the
  // `readPackage` hooks, so the manifest that hook saw is the rewritten one. Asking the same
  // manifest keeps this answer and the override that was actually applied in agreement.
  const overriddenDependencyMatcherFor = createOverriddenDependencyMatcher(opts.parsedOverrides, opts.lockfileDir)
  if (overriddenDependencyMatcherFor != null) {
    for (const project of projects) {
      if (!project.updatePackageManifest) continue
      project.isOverriddenDependency = overriddenDependencyMatcherFor(project.manifest)
    }
  }

  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'resolution_started',
  })
}

function addMissingImporters (ctx: PnpmContext, projects: ImporterToUpdate[]): void {
  ctx.wantedLockfile.importers = ctx.wantedLockfile.importers || {}
  for (const { id } of projects) {
    if (!ctx.wantedLockfile.importers[id]) {
      ctx.wantedLockfile.importers[id] = { specifiers: {} }
    }
  }
}

function pruneUnselectedImporters (ctx: PnpmContext, projects: ImporterToUpdate[]): void {
  const projectIds = new Set(projects.map(({ id }) => id))
  for (const wantedImporter of Object.keys(ctx.wantedLockfile.importers) as ProjectId[]) {
    if (!projectIds.has(wantedImporter)) {
      delete ctx.wantedLockfile.importers[wantedImporter]
    }
  }
}

async function writeLockfileWithoutLinking (
  ctx: PnpmContext,
  { isInstallationOnlyForLockfileCheck, newLockfile, opts }: {
    isInstallationOnlyForLockfileCheck: boolean
    newLockfile: LockfileObject
    opts: InstallInContextOptions
  }
): Promise<void> {
  if (opts.useLockfile && opts.saveLockfile && !isInstallationOnlyForLockfileCheck) {
    await writeWantedLockfileAndRecordVerified({
      lockfileDir: ctx.lockfileDir,
      lockfile: newLockfile,
      cacheDir: opts.cacheDir,
      resolutionVerifiers: opts.resolutionVerifiers,
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
  }

  if (opts.nodeLinker !== 'hoisted' && opts.runPacquet == null && !opts.materializeAfterResolution) {
    // This is only needed because otherwise the reporter will hang.
    // Skipped when pacquet is about to take over the materialization
    // phase: the default reporter completes the progress stream for
    // this prefix on `importing_done`, so emitting it from the
    // lockfileOnly resolve pass would prematurely close the stream
    // and pacquet's own `importing_started` / progress events would
    // render to a stale stream. Pacquet emits its own
    // `importing_done` after the install, which closes the stream
    // normally.
    stageLogger.debug({
      prefix: opts.lockfileDir,
      stage: 'importing_done',
    })
  }
}

async function listDepsRequiringBuild (resolution: GraphResolution, opts: InstallInContextOptions): Promise<DepPath[]> {
  const depsRequiringBuild: DepPath[] = []
  if (!opts.returnListOfDepsRequiringBuild) return depsRequiringBuild
  await Promise.all(Object.entries(resolution.dependenciesGraph).map(async ([depPath, node]) => {
    if (node?.fetching == null) return // We cannot detect if a skipped optional dependency requires build
    const { files } = await node.fetching()
    if (files.requiresBuild) {
      depsRequiringBuild.push(depPath as DepPath)
    }
  }))
  return depsRequiringBuild
}

function reportResolutionOutcome (
  resolution: GraphResolution,
  { opts, originalLockfileForCheck }: { opts: InstallInContextOptions, originalLockfileForCheck: LockfileObject | null }
): void {
  reportPeerDependencyIssues(resolution.peerDependencyIssuesByProjects, {
    lockfileDir: opts.lockfileDir,
    strictPeerDependencies: opts.strictPeerDependencies,
    rules: opts.peerDependencyRules,
  })

  // Skipped when pacquet will take over the materialization. The
  // default reporter's `reportSummary` `take(1)`s the first summary
  // event and combines it with whatever `pkgsDiff` it has at that
  // moment — which is empty here, since pacquet hasn't emitted its
  // per-direct-dep `pnpm:root` events yet. Letting pnpm fire summary
  // now would lock in an empty diff. Pacquet emits its own
  // `pnpm:summary` after the install completes, by which point its
  // root events have populated the diff.
  if (!opts.omitSummaryLog && opts.runPacquet == null) {
    summaryLogger.debug({ prefix: opts.lockfileDir })
  }

  // Similar to the sequencing for when the original wanted lockfile is
  // copied, the new lockfile passed here should be as close as possible to
  // what will eventually be written to disk. Ex: peers should be resolved,
  // the afterAllResolved hook has been applied, etc.
  if (originalLockfileForCheck != null) {
    opts.lockfileCheck?.(originalLockfileForCheck, resolution.newLockfile)
  }
}
