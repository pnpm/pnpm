import path from 'node:path'

import { LAYOUT_VERSION } from '@pnpm/constants'
import { writeModulesManifest } from '@pnpm/installing.modules-yaml'
import { type LockfileObject, writeCurrentLockfile, writeLockfiles } from '@pnpm/lockfile.fs'
import type { HoistedDependencies, IgnoredBuilds } from '@pnpm/types'

import type { HeadlessContext, HeadlessDepGraph } from './context.js'
import { getInjectedDeps } from './extendProjectsWithTargetDirs.js'
import { linkBinsOfImporters } from './linkBinsOfImporters.js'

export interface InstallOutcome {
  ignoredBuilds: IgnoredBuilds | undefined
  newHoistedDependencies?: HoistedDependencies
}

/**
 * Links the bins of the importers and records the installation in the
 * modules manifest and the current lockfile.
 */
export async function writeModulesDirState (ctx: HeadlessContext, depGraph: HeadlessDepGraph, outcome: InstallOutcome): Promise<void> {
  const { opts } = ctx
  if (opts.enableModulesDir === false) return
  /** Skip linking and due to no project manifest */
  if (!ctx.skipPostImportLinking && !opts.ignorePackageManifest) {
    await linkBinsOfImporters(ctx, depGraph)
  }
  await writeModulesManifestOfInstall(ctx, depGraph, outcome)
  await writeLockfilesOfInstall(ctx, depGraph.filteredLockfile)
}

async function writeModulesManifestOfInstall (ctx: HeadlessContext, depGraph: HeadlessDepGraph, outcome: InstallOutcome): Promise<void> {
  const { opts } = ctx
  const injectedDeps = getInjectedDeps(depGraph.injectionTargetsByDepPath, opts.lockfileDir)
  await writeModulesManifest(ctx.rootModulesDir, {
    hoistedDependencies: outcome.newHoistedDependencies!,
    hoistPattern: opts.hoistPattern,
    included: opts.include,
    injectedDeps,
    ignoredBuilds: outcome.ignoredBuilds,
    layoutVersion: LAYOUT_VERSION,
    hoistedLocations: depGraph.hoistedLocations,
    nodeLinker: opts.nodeLinker,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: opts.pendingBuilds,
    publicHoistPattern: opts.publicHoistPattern,
    prunedAt: opts.pruneVirtualStore === true || opts.prunedAt == null
      ? new Date().toUTCString()
      : opts.prunedAt,
    skipped: Array.from(ctx.skipped),
    storeDir: opts.storeDir,
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    allowBuilds: opts.allowBuilds,
    virtualStoreOnly: opts.virtualStoreOnly,
  })
}

async function writeLockfilesOfInstall ({ opts, rootModulesDir, wantedLockfile }: HeadlessContext, filteredLockfile: LockfileObject): Promise<void> {
  const currentLockfileDir = path.join(rootModulesDir, '.pnpm')
  if (opts.useLockfile) {
    // We need to write the wanted lockfile as well.
    // Even though it will only be changed if the workspace will have new projects with no dependencies.
    await writeLockfiles({
      wantedLockfileDir: opts.lockfileDir,
      currentLockfileDir,
      wantedLockfile,
      currentLockfile: filteredLockfile,
    })
  } else {
    await writeCurrentLockfile(currentLockfileDir, filteredLockfile)
  }
}
