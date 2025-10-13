import path from 'path'
import { PnpmError } from '@pnpm/error'
import { linkBins, linkBinsOfPackages } from '@pnpm/link-bins'
import { logger as createLogger } from '@pnpm/logger'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type DependencyManifest } from '@pnpm/types'
import { findWorkspacePackagesNoCheck } from '@pnpm/workspace.find-packages'
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
  const patchers = await DirPatcher.fromMultipleTargets(
    pkgRootDir,
    targetDirs.map(targetDir => path.resolve(opts.workspaceDir!, targetDir))
  )
  await Promise.all(patchers.map(patcher => patcher.apply()))

  // After syncing files, also sync bin links if the package has binaries
  await syncBinLinks(pkgRootDir, targetDirs, opts.workspaceDir)
}

async function syncBinLinks (
  pkgRootDir: string,
  targetDirs: string[],
  workspaceDir: string
): Promise<void> {
  const manifest = await safeReadPackageJsonFromDir(pkgRootDir) as DependencyManifest | undefined

  if (!manifest?.bin || !manifest?.name) {
    return
  }

  // Step 1: Link bins in .pnpm virtual store
  const binLinkPromises = targetDirs.map(async (targetDir) => {
    const resolvedTargetDir = path.resolve(workspaceDir, targetDir)
    const parentNodeModulesDir = path.dirname(resolvedTargetDir)
    const binDir = path.join(parentNodeModulesDir, '.bin')

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
  const allProjects = await findWorkspacePackagesNoCheck(workspaceDir, {})

  const consumerLinkPromises = allProjects.map(async (project) => {
    const projectNodeModules = path.join(project.rootDir, 'node_modules')
    const projectBinDir = path.join(projectNodeModules, '.bin')

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
