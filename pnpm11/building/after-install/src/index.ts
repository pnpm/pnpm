import { LAYOUT_VERSION } from '@pnpm/constants'
import {
  PROJECT_INSTALL_STAGES as EXEC_PROJECT_INSTALL_STAGES,
  runLifecycleHooksConcurrently,
} from '@pnpm/exec.lifecycle'
import type { PnpmContext } from '@pnpm/installing.context'
import { writeModulesManifest } from '@pnpm/installing.modules-yaml'
import { streamParser } from '@pnpm/logger'
import { createStoreController } from '@pnpm/store.connection-manager'
import type {
  DepPath,
  IgnoredBuilds,
  ProjectManifest,
  ProjectRootDir,
} from '@pnpm/types'

import {
  type BuildOptions,
  extendBuildOptions,
  type StrictBuildOptions,
} from './extendBuildOptions.js'
import { getRebuildContext } from './getRebuildContext.js'
import { rebuildPackages } from './rebuildPackages.js'
import { findPackages, parsePackageSelectors } from './selectPackages.js'

export type { BuildOptions }

export const PROJECT_INSTALL_STAGES = ['preinstall', 'install', 'postinstall', 'prepublish']
export const PROJECT_LIFECYCLE_STAGES = ['preinstall', 'install', 'postinstall', 'prepublish', 'prepare']

type ProjectToBuild = { buildIndex: number, manifest: ProjectManifest, rootDir: ProjectRootDir }

export async function buildSelectedPkgs (
  projects: ProjectToBuild[],
  pkgSpecs: string[],
  maybeOpts: BuildOptions
): Promise<{ ignoredBuilds?: IgnoredBuilds }> {
  streamToReporter(maybeOpts)
  const opts = await extendBuildOptions(maybeOpts)
  const ctx = await getRebuildContext(projects, opts)

  if (ctx.currentLockfile?.packages == null) return {}
  const packages = ctx.currentLockfile.packages

  const searched = parsePackageSelectors(packages, pkgSpecs)
  const pkgs = projects.flatMap(({ rootDir }) => findPackages(packages, searched, { prefix: rootDir }))

  const { ignoredPkgs } = await rebuildPackages(
    {
      pkgsToRebuild: new Set(pkgs),
      ...ctx,
    },
    opts
  )
  await writeModulesManifest(ctx.rootModulesDir, {
    prunedAt: new Date().toUTCString(),
    ...ctx.modulesFile,
    hoistedDependencies: ctx.hoistedDependencies,
    hoistPattern: ctx.hoistPattern,
    included: ctx.include,
    ignoredBuilds: mergeIgnoredBuilds(ctx.modulesFile?.ignoredBuilds, ignoredPkgs, pkgs),
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    publicHoistPattern: ctx.publicHoistPattern,
    skipped: Array.from(ctx.skipped),
    storeDir: ctx.modulesFile?.storeDir ?? ctx.storeDir,
    virtualStoreDir: ctx.modulesFile?.virtualStoreDir ?? ctx.virtualStoreDir,
    virtualStoreDirMaxLength: ctx.modulesFile?.virtualStoreDirMaxLength ?? ctx.virtualStoreDirMaxLength,
    allowBuilds: opts.allowBuilds,
  })
  return {
    ignoredBuilds: ignoredPkgs,
  }
}

function streamToReporter (maybeOpts: BuildOptions): void {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
}

export async function buildProjects (
  projects: ProjectToBuild[],
  maybeOpts: BuildOptions
): Promise<void> {
  streamToReporter(maybeOpts)
  const opts = await extendBuildOptions(maybeOpts)
  const ctx = await getRebuildContext(projects, opts)

  const { pkgsThatWereRebuilt, ignoredPkgs } = await rebuildPackages(
    {
      pkgsToRebuild: new Set(getDepPathsToRebuild(ctx, opts)),
      ...ctx,
    },
    opts
  )

  ctx.pendingBuilds = ctx.pendingBuilds.filter((depPath) => !pkgsThatWereRebuilt.has(depPath))

  await runProjectLifecycleHooks(ctx, opts)
  for (const { id, manifest } of Object.values(ctx.projects)) {
    if (((manifest?.scripts) != null) && (!opts.pending || ctx.pendingBuilds.includes(id))) {
      ctx.pendingBuilds.splice(ctx.pendingBuilds.indexOf(id), 1)
    }
  }

  await writeModulesManifest(ctx.rootModulesDir, {
    prunedAt: new Date().toUTCString(),
    ...ctx.modulesFile,
    hoistedDependencies: ctx.hoistedDependencies,
    hoistPattern: ctx.hoistPattern,
    included: ctx.include,
    ignoredBuilds: ignoredPkgs,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    publicHoistPattern: ctx.publicHoistPattern,
    allowBuilds: opts.allowBuilds,
    skipped: Array.from(ctx.skipped),
    storeDir: ctx.storeDir,
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
  })
}

function getDepPathsToRebuild (ctx: PnpmContext, opts: StrictBuildOptions): string[] {
  if (opts.pending) return ctx.pendingBuilds
  if ((ctx.currentLockfile?.packages) != null) return Object.keys(ctx.currentLockfile.packages)
  return []
}

async function runProjectLifecycleHooks (ctx: PnpmContext, opts: StrictBuildOptions): Promise<void> {
  const store = await createStoreController(opts)
  const scriptsOpts = {
    extraBinPaths: ctx.extraBinPaths,
    extendNodePath: opts.extendNodePath,
    extraNodePaths: ctx.extraNodePaths,
    extraEnv: opts.extraEnv,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    storeController: store.ctrl,
    unsafePerm: opts.unsafePerm || false,
    userAgent: opts.userAgent,
  }
  await runLifecycleHooksConcurrently({
    childConcurrency: opts.childConcurrency || 5,
    importers: Object.values(ctx.projects),
    opts: scriptsOpts,
    projectDependencies: opts.projectDependencies,
    stages: opts.stages ?? (opts.deploy
      ? EXEC_PROJECT_INSTALL_STAGES
      : (ctx.include?.devDependencies !== false
        ? PROJECT_LIFECYCLE_STAGES
        : PROJECT_INSTALL_STAGES)),
  })
}

/**
 * Merge new ignoredBuilds from a selective rebuild with existing ones.
 * Keeps existing entries for packages that weren't part of this rebuild.
 */
function mergeIgnoredBuilds (
  existing: IgnoredBuilds | undefined,
  newIgnored: IgnoredBuilds,
  rebuiltPkgs: DepPath[]
): IgnoredBuilds | undefined {
  if (!existing?.size && !newIgnored.size) return undefined
  const rebuiltSet = new Set<DepPath>(rebuiltPkgs)
  const merged = new Set<DepPath>()
  if (existing) {
    for (const depPath of existing) {
      if (!rebuiltSet.has(depPath)) {
        merged.add(depPath)
      }
    }
  }
  for (const depPath of newIgnored) {
    merged.add(depPath)
  }
  return merged.size ? merged : undefined
}
