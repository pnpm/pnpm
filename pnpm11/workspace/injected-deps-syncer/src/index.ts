import fs from 'node:fs'
import path from 'node:path'

import { linkBins, linkBinsOfPackages } from '@pnpm/bins.linker'
import { removeBin } from '@pnpm/bins.remover'
import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import { isError, PnpmError } from '@pnpm/error'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import { logger as createLogger } from '@pnpm/logger'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DependencyManifest, ProjectManifest } from '@pnpm/types'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import normalizePath from 'normalize-path'

import { DirPatcher, publishEditsForWatchers, type PublishSource, readPublishSource } from './DirPatcher.js'

interface SkipSyncInjectedDepsMessage {
  message: string
  reason: 'no-name' | 'no-injected-deps'
  opts: SyncInjectedDepsOptions
}

const logger = createLogger<SkipSyncInjectedDepsMessage>('skip-sync-injected-deps')

interface WatchPublishError {
  err: unknown
  message: string
}

const watchLogger = createLogger<WatchPublishError>('sync-injected-deps-watch')

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

export interface InjectedEditWatch {
  stop: () => Promise<void>
}

/**
 * The source directory and injected copies a running script should publish.
 * `undefined` when the package has no name, no workspace, or no injected copies,
 * or when the modules manifest cannot be read. The sync after the script
 * reports that error, so it does not stop the script from starting.
 */
export async function injectedEditDirs (
  opts: SyncInjectedDepsOptions
): Promise<{ sourceDir: string, targetDirs: string[] } | undefined> {
  if (!opts.pkgName || opts.workspaceDir == null) return undefined
  let located: Awaited<ReturnType<typeof readInjectedTargets>>
  try {
    located = await readInjectedTargets(opts.workspaceDir, opts.pkgRootDir)
  } catch (err: unknown) {
    watchLogger.debug({
      err,
      message: `Not publishing injected dependencies of ${opts.pkgRootDir} while its script is running`,
    })
    return undefined
  }
  if (located?.resolvedTargetDirs == null) return undefined
  return { sourceDir: located.pkgRootDir, targetDirs: located.resolvedTargetDirs }
}

/**
 * Publish injected copies about every 200ms until `stop`. `editedSinceMs` stays
 * two seconds behind the start of the watch, which covers one-second mtime
 * resolution. Stop waits for an in-flight publish so the end-of-script hardlink
 * sync does not run beside it.
 */
export function watchInjectedEdits (sourceDir: string, targetDirs: string[]): InjectedEditWatch {
  const editedSinceMs = Date.now() - 2000
  let stopped = false
  let timer: ReturnType<typeof setInterval> | undefined
  let inFlight: Promise<void> | undefined

  const publish = (): void => {
    if (stopped || inFlight != null) return
    inFlight = publishToTargets(sourceDir, targetDirs, editedSinceMs).then(() => {
      inFlight = undefined
    })
  }

  publish()
  timer = setInterval(publish, 200)
  return {
    stop: async () => {
      stopped = true
      if (timer != null) clearInterval(timer)
      await inFlight
    },
  }
}

async function publishToTargets (sourceDir: string, targetDirs: string[], editedSinceMs: number): Promise<void> {
  let source: PublishSource
  try {
    source = await readPublishSource(sourceDir)
  } catch (err: unknown) {
    watchLogger.debug({
      err,
      message: `Failed to read injected dependency ${sourceDir} while its script is running`,
    })
    return
  }
  await Promise.all(targetDirs.map(async targetDir => {
    try {
      await publishEditsForWatchers(source, targetDir, editedSinceMs)
    } catch (err: unknown) {
      watchLogger.debug({
        err,
        message: `Failed to publish injected dependency ${targetDir} while its script is running`,
      })
    }
  }))
}

export async function syncInjectedDeps (opts: SyncInjectedDepsOptions): Promise<void> {
  const located = await locateInjectedTargets(opts)
  if (located == null) return

  const { modules, pkgRootDir, resolvedTargetDirs } = located
  const patchers = await DirPatcher.fromMultipleTargets(pkgRootDir, resolvedTargetDirs)
  await Promise.all(patchers.map(patcher => patcher.apply()))

  await syncInjectedBinLinks(opts, modules, pkgRootDir, resolvedTargetDirs)
}

async function locateInjectedTargets (opts: SyncInjectedDepsOptions): Promise<{
  modules: NonNullable<Awaited<ReturnType<typeof readModulesManifest>>>
  pkgRootDir: string
  resolvedTargetDirs: string[]
} | undefined> {
  if (!opts.pkgName) {
    logger.debug({
      reason: 'no-name',
      message: `Skipping sync of ${opts.pkgRootDir} as an injected dependency because, without a name, it cannot be a dependency`,
      opts,
    })
    return undefined
  }
  if (!opts.workspaceDir) {
    throw new PnpmError('NO_WORKSPACE_DIR', 'Cannot update injected dependencies without workspace dir')
  }
  const located = await readInjectedTargets(opts.workspaceDir, opts.pkgRootDir)
  if (located?.resolvedTargetDirs == null) {
    logger.debug({
      reason: 'no-injected-deps',
      message: located == null
        ? 'Skipping sync of injected dependencies because none were detected'
        : `There are no injected dependencies from ${opts.pkgRootDir}`,
      opts,
    })
    return undefined
  }
  return { ...located, resolvedTargetDirs: located.resolvedTargetDirs }
}

async function syncInjectedBinLinks (
  opts: SyncInjectedDepsOptions,
  modules: NonNullable<Awaited<ReturnType<typeof readModulesManifest>>>,
  pkgRootDir: string,
  resolvedTargetDirs: string[]
): Promise<void> {
  const hoistedBinDir = modules.virtualStoreDir == null
    ? undefined
    : path.join(path.resolve(opts.workspaceDir!, modules.virtualStoreDir), 'node_modules', '.bin')
  const previousBinNames = opts.manifestBeforeScripts == null
    ? []
    : (await getBinsFromPackageManifest(opts.manifestBeforeScripts, pkgRootDir)).map(command => command.name)

  await syncBinLinks({
    hoistedBinDir,
    pkgRootDir,
    previousBinNames,
    resolvedTargetDirs,
    workspaceDir: opts.workspaceDir!,
  })
}

export interface SyncInjectedDepsOfModulesDirOptions {
  /** Selects the source files the install put in the copies, so the sync adds no file the install left out. */
  includeOnlyPackageFiles?: boolean
  /** The directory whose lockfile lists the injected dependencies. */
  lockfileDir: string
  /** The modules directory that holds the `.modules.yaml` of `lockfileDir`. */
  modulesDir: string
  /** The sources whose copies are synced. Copies of other sources keep their contents. */
  sourceDirs: ReadonlySet<string>
}

/**
 * Brings the injected copies listed in one modules directory back in step
 * with their sources. A project with its own lockfile injects copies of
 * workspace projects whose lifecycle scripts run in their own install, so
 * nothing else syncs those copies after the scripts run.
 */
export async function syncInjectedDepsOfModulesDir (opts: SyncInjectedDepsOfModulesDirOptions): Promise<void> {
  const modules = await readModulesManifest(opts.modulesDir)
  if (!modules?.injectedDeps) return
  await Promise.all(Object.entries(modules.injectedDeps).map(async ([sourceId, targetDirs]) => {
    const sourceDir = path.resolve(opts.lockfileDir, sourceId)
    if (!opts.sourceDirs.has(sourceDir) || targetDirs.length === 0) return
    // A publish directory the build did not produce leaves the copies alone
    // rather than emptying them.
    if (!await dirExists(sourceDir)) return
    const resolvedTargetDirs = targetDirs.map((targetDir) => path.resolve(opts.lockfileDir, targetDir))
    const patchers = await DirPatcher.fromMultipleTargets(sourceDir, resolvedTargetDirs, opts.includeOnlyPackageFiles)
    await Promise.all(patchers.map(patcher => patcher.apply()))
  }))
}

async function dirExists (dir: string): Promise<boolean> {
  try {
    await fs.promises.stat(dir)
    return true
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}

async function readInjectedTargets (workspaceDir: string, pkgRootDirInput: string): Promise<{
  modules: NonNullable<Awaited<ReturnType<typeof readModulesManifest>>>
  pkgRootDir: string
  resolvedTargetDirs: string[] | undefined
} | undefined> {
  const pkgRootDir = path.resolve(workspaceDir, pkgRootDirInput)
  const modules = await readModulesManifest(path.resolve(workspaceDir, 'node_modules'))
  if (modules?.injectedDeps == null) return undefined
  const injectedDepKey = normalizePath(path.relative(workspaceDir, pkgRootDir), true)
  const targetDirs = modules.injectedDeps[injectedDepKey]
  return {
    modules,
    pkgRootDir,
    resolvedTargetDirs: targetDirs == null || targetDirs.length === 0
      ? undefined
      : targetDirs.map(targetDir => path.resolve(workspaceDir, targetDir)),
  }
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

  const binLinkPromises = opts.resolvedTargetDirs.map(async resolvedTargetDir => {
    await linkInjectedTargetBins(resolvedTargetDir, manifest, staleBinNames, opts.hoistedBinDir)
  })

  const allProjects = await findWorkspaceProjectsNoCheck(opts.workspaceDir, {})
  const consumerLinkPromises = allProjects.map(async project => {
    await relinkWorkspaceConsumerBins(project, staleBinNames)
  })

  await Promise.all([...binLinkPromises, ...consumerLinkPromises])
}

async function linkInjectedTargetBins (
  resolvedTargetDir: string,
  manifest: DependencyManifest,
  staleBinNames: string[],
  hoistedBinDir: string | undefined
): Promise<void> {
  const binDir = path.join(path.dirname(resolvedTargetDir), '.bin')
  const binDirs = [binDir, path.join(resolvedTargetDir, 'node_modules', '.bin')]
  if (hoistedBinDir != null) binDirs.push(hoistedBinDir)

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
}

async function relinkWorkspaceConsumerBins (
  project: { rootDir: string, manifest: ProjectManifest },
  staleBinNames: string[]
): Promise<void> {
  const projectNodeModules = path.join(project.rootDir, 'node_modules')
  const projectBinDir = path.join(projectNodeModules, '.bin')

  await Promise.all(staleBinNames.map(async name => removeBin(path.join(projectBinDir, name))))

  await linkBins(projectNodeModules, projectBinDir, {
    allowExoticManifests: true,
    projectManifest: project.manifest,
    warn: (msg: string) => {
      console.warn(`[linkBins warning] ${msg}`)
    },
  })
}
