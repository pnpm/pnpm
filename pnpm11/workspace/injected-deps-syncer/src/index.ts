import path from 'node:path'

import { linkBins, linkBinsOfPackages } from '@pnpm/bins.linker'
import { removeBin } from '@pnpm/bins.remover'
import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import { PnpmError } from '@pnpm/error'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import { logger as createLogger } from '@pnpm/logger'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DependencyManifest } from '@pnpm/types'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import normalizePath from 'normalize-path'

import { DirPatcher } from './DirPatcher.js'

interface SkipSyncInjectedDepsMessage {
  message: string
  reason: 'no-name' | 'no-injected-deps'
  opts: SyncInjectedDepsOptions
}

const logger = createLogger<SkipSyncInjectedDepsMessage>('skip-sync-injected-deps')

export interface SyncInjectedDepsOptions {
  pkgName: string | undefined
  pkgRootDir: string
  workspaceDir: string | undefined
  /**
   * The package's manifest as it was before the scripts ran. A script that
   * drops a bin leaves its shim behind, and the copies cannot say which bins
   * they used to have: their `package.json` is hardlinked to the source, so
   * an in-place rewrite has already reached them.
   */
  manifestBeforeScripts?: DependencyManifest
}

export async function syncInjectedDeps (opts: SyncInjectedDepsOptions): Promise<void> {
  if (!opts.pkgName) {
    logger.debug({
      reason: 'no-name',
      message: `Skipping sync of ${opts.pkgRootDir} as an injected dependency because, without a name, it cannot be a dependency`,
      opts,
    })
    return
  }
  if (!opts.workspaceDir) {
    throw new PnpmError('NO_WORKSPACE_DIR', 'Cannot update injected dependencies without workspace dir')
  }
  const pkgRootDir = path.resolve(opts.workspaceDir, opts.pkgRootDir)
  const modulesDir = path.resolve(opts.workspaceDir, 'node_modules')
  const modules = await readModulesManifest(modulesDir)
  if (!modules?.injectedDeps) {
    logger.debug({
      reason: 'no-injected-deps',
      message: 'Skipping sync of injected dependencies because none were detected',
      opts,
    })
    return
  }
  const injectedDepKey = normalizePath(path.relative(opts.workspaceDir, pkgRootDir), true)
  const targetDirs: string[] | undefined = modules.injectedDeps[injectedDepKey]
  if (!targetDirs || targetDirs.length === 0) {
    logger.debug({
      reason: 'no-injected-deps',
      message: `There are no injected dependencies from ${opts.pkgRootDir}`,
      opts,
    })
    return
  }
  const resolvedTargetDirs = targetDirs.map(targetDir => path.resolve(opts.workspaceDir!, targetDir))
  const patchers = await DirPatcher.fromMultipleTargets(pkgRootDir, resolvedTargetDirs)

  await Promise.all(patchers.map(patcher => patcher.apply()))

  await syncBinLinks({
    // The install hoists bins into the virtual store's own `.bin` as well.
    hoistedBinDir: modules.virtualStoreDir == null
      ? undefined
      : path.join(path.resolve(opts.workspaceDir, modules.virtualStoreDir), 'node_modules', '.bin'),
    pkgRootDir,
    previousBinNames: opts.manifestBeforeScripts == null
      ? []
      : (await getBinsFromPackageManifest(opts.manifestBeforeScripts, pkgRootDir)).map(command => command.name),
    resolvedTargetDirs,
    workspaceDir: opts.workspaceDir,
  })
}

/** The commands a package declares, or none when it declares no bins. */
async function readBinNames (pkgDir: string): Promise<string[]> {
  const manifest = await safeReadPackageJsonFromDir(pkgDir) as DependencyManifest | undefined
  if (!manifest?.name) return []
  const commands = await getBinsFromPackageManifest(manifest, pkgDir)
  return commands.map(command => command.name)
}

interface SyncBinLinksOptions {
  hoistedBinDir: string | undefined
  pkgRootDir: string
  previousBinNames: string[]
  resolvedTargetDirs: string[]
  workspaceDir: string
}

async function syncBinLinks (opts: SyncBinLinksOptions): Promise<void> {
  const manifest = await safeReadPackageJsonFromDir(opts.pkgRootDir) as DependencyManifest | undefined

  if (!manifest?.name) {
    return
  }

  // A script can drop a bin as easily as it can add one. `linkBins` only ever
  // creates shims, so without this the shim for a dropped bin survives and
  // points at a command that is no longer there.
  const currentBinNames = new Set(await readBinNames(opts.pkgRootDir))
  const staleBinNames = opts.previousBinNames.filter(name => !currentBinNames.has(name))

  // Step 1: Link bins in .pnpm virtual store
  const binLinkPromises = opts.resolvedTargetDirs.map(async (resolvedTargetDir) => {
    const parentNodeModulesDir = path.dirname(resolvedTargetDir)
    const binDir = path.join(parentNodeModulesDir, '.bin')

    // The installer writes an injected package's own bins inside the copy,
    // while this function writes them beside it. A dropped bin has to be
    // cleared from both, or the one this function never wrote survives.
    const binDirs = [binDir, path.join(resolvedTargetDir, 'node_modules', '.bin')]
    if (opts.hoistedBinDir != null) binDirs.push(opts.hoistedBinDir)
    await Promise.all(binDirs.flatMap(
      dir => staleBinNames.map(async name => removeBin(path.join(dir, name)))
    ))

    if (manifest.bin == null) return
    await linkBinsOfPackages(
      [{
        manifest,
        location: resolvedTargetDir,
      }],
      binDir,
      {}
    )
  })

  // Step 2: Relink bins for all workspace projects
  // We need to relink bins for all workspace projects because injected deps
  // can be used by any project in the workspace. We relink all bins (not just
  // this package) to ensure consistency.
  const allProjects = await findWorkspaceProjectsNoCheck(opts.workspaceDir, {})

  const consumerLinkPromises = allProjects.map(async (project) => {
    const projectNodeModules = path.join(project.rootDir, 'node_modules')
    const projectBinDir = path.join(projectNodeModules, '.bin')

    // A stale name another package legitimately owns is put back by the
    // relink below, so removing first costs nothing and catches the shim
    // this package left behind.
    await Promise.all(staleBinNames.map(async name => removeBin(path.join(projectBinDir, name))))

    // Relink all bins in the project's node_modules
    await linkBins(projectNodeModules, projectBinDir, {
      allowExoticManifests: true,
      projectManifest: project.manifest,
      warn: (msg: string) => {
        console.warn(`[linkBins warning] ${msg}`)
      },
    })
  })

  await Promise.all([...binLinkPromises, ...consumerLinkPromises])
}
