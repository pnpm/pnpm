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
  /** Selects the source files the install put in the copies, so the sync adds no file the install left out. */
  includeOnlyPackageFiles?: boolean
  /**
   * Set when each project has its own lockfile (`sharedWorkspaceLockfile:
   * false`). Each project's own `.modules.yaml` then lists the copies injected
   * into it, so every project's is read besides the workspace root's.
   */
  projectLockfiles?: ProjectLockfiles
}

/** How to find the workspace projects and the modules directory each one installs into. */
export interface ProjectLockfiles {
  workspacePackagePatterns?: string[]
  modulesDir?: string
  modulesDirsByProjectName?: Record<string, string>
}

/** An injected copy, and the `.bin` its install hoists the virtual store's bins into. */
interface InjectedCopy {
  dir: string
  hoistedBinDir: string | undefined
}

/** A directory the install injects copies of, and those copies. */
interface InjectedSource {
  sourceDir: string
  copies: InjectedCopy[]
}

export interface InjectedEditWatch {
  stop: () => Promise<void>
}

/**
 * The source directories and injected copies a running script should publish,
 * the same ones `syncInjectedDeps` patches. Empty when the package has no name,
 * no workspace, or no injected copies, or when the modules manifest cannot be
 * read. The sync after the script reports that error, so it does not stop the
 * script from starting.
 */
export async function injectedEditDirs (
  opts: SyncInjectedDepsOptions
): Promise<Array<{ sourceDir: string, targetDirs: string[] }>> {
  if (!opts.pkgName || opts.workspaceDir == null) return []
  let located: Awaited<ReturnType<typeof readInjectedTargets>>
  try {
    located = await readInjectedTargets({ ...opts, workspaceDir: opts.workspaceDir })
  } catch (err: unknown) {
    watchLogger.debug({
      err,
      message: `Not publishing injected dependencies of ${opts.pkgRootDir} while its script is running`,
    })
    return []
  }
  return (located?.sources ?? []).map(({ sourceDir, copies }) => ({ sourceDir, targetDirs: copies.map(({ dir }) => dir) }))
}

/**
 * Publish injected copies about every 200ms until `stop`. `editedSinceMs` stays
 * two seconds behind the start of the watch, which covers one-second mtime
 * resolution. Stop waits for an in-flight publish so the end-of-script hardlink
 * sync does not run beside it.
 */
export function watchInjectedEdits (sourceDir: string, targetDirs: string[], includeOnlyPackageFiles?: boolean): InjectedEditWatch {
  const editedSinceMs = Date.now() - 2000
  let stopped = false
  let timer: ReturnType<typeof setInterval> | undefined
  let inFlight: Promise<void> | undefined

  const publish = (): void => {
    if (stopped || inFlight != null) return
    inFlight = publishToTargets({ sourceDir, targetDirs, includeOnlyPackageFiles }, editedSinceMs).then(() => {
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

async function publishToTargets (
  { sourceDir, targetDirs, includeOnlyPackageFiles }: { sourceDir: string, targetDirs: string[], includeOnlyPackageFiles?: boolean },
  editedSinceMs: number
): Promise<void> {
  let source: PublishSource
  try {
    // A publish directory the script has not built yet reads back empty,
    // which would empty the injected copies.
    if (!await dirExists(sourceDir)) return
    source = await readPublishSource(sourceDir, includeOnlyPackageFiles)
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

  for (const source of located.sources) {
    await syncSource(opts, located.pkgRootDir, source) // eslint-disable-line no-await-in-loop -- each relinks every project's bins
  }
}

async function syncSource (opts: SyncInjectedDepsOptions, pkgRootDir: string, source: InjectedSource): Promise<void> {
  // A publish directory the script did not build leaves the copies alone
  // rather than emptying them.
  if (!await dirExists(source.sourceDir)) return
  const patchers = await DirPatcher.fromMultipleTargets(source.sourceDir, source.copies.map(({ dir }) => dir), opts.includeOnlyPackageFiles)
  await Promise.all(patchers.map(patcher => patcher.apply()))
  await syncInjectedBinLinks(opts, pkgRootDir, source)
}

async function locateInjectedTargets (opts: SyncInjectedDepsOptions): Promise<{
  pkgRootDir: string
  sources: InjectedSource[]
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
  const located = await readInjectedTargets({ ...opts, workspaceDir: opts.workspaceDir })
  if (located == null || located.sources.length === 0) {
    logger.debug({
      reason: 'no-injected-deps',
      message: located == null
        ? 'Skipping sync of injected dependencies because none were detected'
        : `There are no injected dependencies from ${opts.pkgRootDir}`,
      opts,
    })
    return undefined
  }
  return located
}

async function syncInjectedBinLinks (
  opts: SyncInjectedDepsOptions,
  pkgRootDir: string,
  { sourceDir, copies }: InjectedSource
): Promise<void> {
  const previousBinNames = opts.manifestBeforeScripts == null
    ? []
    : (await getBinsFromPackageManifest(opts.manifestBeforeScripts, pkgRootDir)).map(command => command.name)

  await syncBinLinks({
    copies,
    pkgRootDir: sourceDir,
    previousBinNames,
    workspaceDir: opts.workspaceDir!,
    projectLockfiles: opts.projectLockfiles,
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

/**
 * The copies that the modules manifests list of `opts.pkgRootDir` and of the
 * `publishConfig.directory` the install injects in its place. A `file:`
 * dependency on the project bypasses the publish directory, so both are
 * looked up. `undefined` when no manifest lists any injected dependency.
 */
async function readInjectedTargets (opts: SyncInjectedDepsOptions & { workspaceDir: string }): Promise<{
  pkgRootDir: string
  sources: InjectedSource[]
} | undefined> {
  const pkgRootDir = path.resolve(opts.workspaceDir, opts.pkgRootDir)
  const manifests = await readModulesManifests(opts)
  if (manifests.every(({ modules }) => modules?.injectedDeps == null)) return undefined
  const publishDir = await readPublishDir(pkgRootDir, opts.manifestBeforeScripts)
  const sourceDirs = publishDir === pkgRootDir ? [pkgRootDir] : [publishDir, pkgRootDir]
  const sources = sourceDirs
    .map((sourceDir) => ({ sourceDir, copies: manifests.flatMap((manifest) => copiesListedIn(manifest, sourceDir)) }))
    .filter(({ copies }) => copies.length > 0)
  return { pkgRootDir, sources }
}

function copiesListedIn (
  { lockfileDir, modules }: Awaited<ReturnType<typeof readModulesManifests>>[number],
  sourceDir: string
): InjectedCopy[] {
  const targetDirs = modules?.injectedDeps?.[normalizePath(path.relative(lockfileDir, sourceDir), true)] ?? []
  const hoistedBinDir = modules?.virtualStoreDir == null
    ? undefined
    : path.join(path.resolve(lockfileDir, modules.virtualStoreDir), 'node_modules', '.bin')
  return targetDirs.map((targetDir) => ({ dir: path.resolve(lockfileDir, targetDir), hoistedBinDir }))
}

/**
 * The modules manifests that list injected copies: the workspace root's, or,
 * with `opts.projectLockfiles`, every project's, the root's included, each
 * under the modules directory that project installs into.
 */
async function readModulesManifests (opts: SyncInjectedDepsOptions & { workspaceDir: string }): Promise<Array<{
  lockfileDir: string
  modules: Awaited<ReturnType<typeof readModulesManifest>>
}>> {
  let modulesDirs = [{ lockfileDir: opts.workspaceDir, modulesDir: path.join(opts.workspaceDir, 'node_modules') }]
  if (opts.projectLockfiles != null) {
    const { modulesDir, modulesDirsByProjectName } = opts.projectLockfiles
    const projects = await findWorkspaceProjects(opts.workspaceDir, opts.projectLockfiles)
    modulesDirs = projects.map(({ rootDir, manifest }) => {
      const projectModulesDir = (manifest.name != null ? modulesDirsByProjectName?.[manifest.name] : undefined) ?? modulesDir ?? 'node_modules'
      return { lockfileDir: rootDir, modulesDir: path.resolve(rootDir, projectModulesDir) }
    })
  }
  return Promise.all(modulesDirs.map(async ({ lockfileDir, modulesDir }) => ({
    lockfileDir,
    modules: await readModulesManifest(modulesDir),
  })))
}

/**
 * The workspace's projects, found with its `packages` patterns. The modules
 * directories keep installed packages from being taken for projects.
 */
async function findWorkspaceProjects (workspaceDir: string, projectLockfiles: ProjectLockfiles | undefined): Promise<Awaited<ReturnType<typeof findWorkspaceProjectsNoCheck>>> {
  return findWorkspaceProjectsNoCheck(workspaceDir, {
    patterns: projectLockfiles?.workspacePackagePatterns,
    modulesDir: projectLockfiles?.modulesDir,
    modulesDirsByProjectName: projectLockfiles?.modulesDirsByProjectName,
  })
}

/** The directory the install injects in place of the project at `pkgRootDir`. */
async function readPublishDir (pkgRootDir: string, manifestBeforeScripts: DependencyManifest | undefined): Promise<string> {
  const manifest = manifestBeforeScripts ?? await safeReadPackageJsonFromDir(pkgRootDir) as ProjectManifest | undefined
  const publishConfig = manifest?.publishConfig
  return publishConfig?.directory != null && publishConfig.linkDirectory !== false
    ? path.join(pkgRootDir, publishConfig.directory)
    : pkgRootDir
}

/** The commands a package declares, or none when it declares no bins. */
async function readBinNames (pkgDir: string): Promise<string[]> {
  const manifest = await safeReadPackageJsonFromDir(pkgDir) as DependencyManifest | undefined
  if (!manifest?.name) return []
  const commands = await getBinsFromPackageManifest(manifest, pkgDir)
  return commands.map(command => command.name)
}

interface SyncBinLinksOptions {
  copies: InjectedCopy[]
  pkgRootDir: string
  previousBinNames: string[]
  workspaceDir: string
  projectLockfiles: ProjectLockfiles | undefined
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

  const binLinkPromises = opts.copies.map(async ({ dir, hoistedBinDir }) => {
    await linkInjectedTargetBins(dir, manifest, staleBinNames, hoistedBinDir)
  })

  const allProjects = await findWorkspaceProjects(opts.workspaceDir, opts.projectLockfiles)
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
