import fs from 'node:fs'
import path from 'node:path'

import type { CommandHandlerMap } from '@pnpm/cli.command'
import { isError } from '@pnpm/error'
import {
  cleanOrphanedInstallDirs,
  getGlobalPackageDetails,
  getInstalledBins,
  isValidGlobalDependencyAlias,
  scanGlobalPackages,
} from '@pnpm/global.packages'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { isSubdir } from 'is-subdir'

import { getSingleLineErrorMessage } from './errorMessage.js'
import { type GlobalAddOptions, installGroup, resolveLocalParam } from './globalAdd.js'
import { isPnpmCliDependency } from './pnpmCliPackages.js'

/**
 * pnpm 10 installed every global package into one project,
 * `<global-dir>/5`, and linked their bins straight into the pnpm home.
 * The current layout keeps each package in its own group under
 * `<global-dir>/v11` and links bins into `<pnpm-home>/bin`, so after an
 * upgrade the packages of the old project are neither on `PATH` nor
 * listed by `list -g`.
 */
const LEGACY_GLOBAL_LAYOUT = '5'

/** A generated shim stays far below this; a larger file is not one. */
const MAX_SHIM_BYTES = 64 * 1024

/**
 * The files pnpm 10 wrote for one bin: the sh shim, and on Windows the
 * `.cmd` and `.ps1` shims beside it, or a hard-linked `.exe`.
 */
const LEGACY_BIN_EXTENSIONS = ['', '.cmd', '.ps1', '.exe']

export type MigrateLegacyGlobalPackagesOptions = GlobalAddOptions & {
  pnpmHomeDir?: string
}

/** The global project of the previous layout, next to the current one. */
export interface LegacyGlobalLayout {
  dir: string
  /**
   * The direct dependencies its manifest records, the pnpm CLI included:
   * its bins are cleaned up like any other, while installing it into the
   * current layout is `pnpm setup`'s job.
   */
  dependencies: Record<string, string>
}

/** The project next to `globalDir`, if its manifest is still there. */
export async function readLegacyGlobalLayout (globalDir: string): Promise<LegacyGlobalLayout | null> {
  const dir = path.join(path.dirname(globalDir), LEGACY_GLOBAL_LAYOUT)
  const manifest = await safeReadPackageJsonFromDir(dir)
  if (manifest == null) return null
  const dependencies: Record<string, string> = {}
  for (const [alias, spec] of Object.entries(manifest.dependencies ?? {})) {
    if (isValidGlobalDependencyAlias(alias)) {
      dependencies[alias] = spec
    }
  }
  return { dir, dependencies }
}

/**
 * One package to migrate. Messages name the alias: the selector can carry
 * the credentials of a tarball or git URL.
 */
export interface MigrationSelector {
  alias: string
  selector: string
}

/**
 * The dependencies no current group installs, each with its `add -g`
 * selector. A local path is anchored at the previous project, which is
 * where pnpm 10 recorded it relative to.
 */
export function legacyMigrationSelectors (
  legacy: LegacyGlobalLayout,
  installedAliases: ReadonlySet<string>
): MigrationSelector[] {
  return packagesToMigrate(legacy)
    .filter(([alias]) => !installedAliases.has(alias))
    .map(([alias, spec]) => ({ alias, selector: `${alias}@${resolveLocalParam(spec, legacy.dir)}` }))
}

/** The dependencies other than the pnpm CLI. */
function packagesToMigrate (legacy: LegacyGlobalLayout): Array<[string, string]> {
  return Object.entries(legacy.dependencies).filter(([alias, spec]) => !isPnpmCliDependency(alias, spec))
}

/**
 * Reinstall the packages of the previous global layout as groups of the
 * current one, then delete the previous project together with the bins it
 * linked into the pnpm home. A package a current group already declares
 * is left alone: reinstalling it would replace that group, the packages
 * installed together with it included. The previous project stays for
 * the next `update -g` while a package failed to install or while such a
 * declared package is still missing its files.
 */
export async function migrateLegacyGlobalPackages (
  opts: MigrateLegacyGlobalPackagesOptions,
  commands: CommandHandlerMap
): Promise<void> {
  const globalDir = opts.globalPkgDir!
  const legacy = await readLegacyGlobalLayout(globalDir)
  if (legacy == null) return
  cleanOrphanedInstallDirs(globalDir)
  const current = await scanCurrentGroups(globalDir)
  const selectors = legacyMigrationSelectors(legacy, current.declared)
  const everyPackageMigrated = await installMigratedGroups(opts, legacy, selectors, commands)
  if (!everyPackageMigrated) {
    globalWarn(`Kept ${legacy.dir} for the packages that failed to migrate. ` +
      'The next "pnpm update -g" retries them; delete the directory to skip them.')
    return
  }
  const awaitingRestore = aliasesAwaitingRestore(legacy, current)
  if (awaitingRestore.length > 0) {
    globalWarn(`Kept ${legacy.dir} until ${awaitingRestore.join(', ')} are installed again: ` +
      'a current global package declares them without their files. "pnpm update -g" restores a global package ' +
      'that lost all of its files; otherwise remove it with "pnpm remove -g". ' +
      'The next "pnpm update -g" then removes the directory.')
    return
  }
  await removeLegacyGlobalLayout(legacy, opts.pnpmHomeDir)
}

/**
 * What the current groups install: every alias they declare, and the
 * subset whose package files are on disk.
 */
interface CurrentGroups {
  declared: ReadonlySet<string>
  materialized: ReadonlySet<string>
}

async function scanCurrentGroups (globalDir: string): Promise<CurrentGroups> {
  const groups = scanGlobalPackages(globalDir)
  const details = await Promise.all(groups.map(getGlobalPackageDetails))
  return {
    declared: new Set(groups.flatMap((pkg) => Object.keys(pkg.dependencies))),
    materialized: new Set(details.flat().map(({ alias }) => alias)),
  }
}

/**
 * The dependencies a current group declares without having their files.
 * Until they are installed again, the previous project holds the only copy.
 */
function aliasesAwaitingRestore (legacy: LegacyGlobalLayout, current: CurrentGroups): string[] {
  return packagesToMigrate(legacy)
    .map(([alias]) => alias)
    .filter((alias) => current.declared.has(alias) && !current.materialized.has(alias))
}

async function installMigratedGroups (
  opts: MigrateLegacyGlobalPackagesOptions,
  legacy: LegacyGlobalLayout,
  selectors: MigrationSelector[],
  commands: CommandHandlerMap
): Promise<boolean> {
  if (selectors.length === 0) return true
  globalInfo(`Migrating global packages from ${legacy.dir}: ${selectors.map(({ alias }) => alias).join(', ')}`)
  let everyPackageMigrated = true
  for (const { alias, selector } of selectors) {
    try {
      // eslint-disable-next-line no-await-in-loop -- groups share the global bin directory, so they are installed one at a time
      await installGroup({
        opts,
        globalDir: opts.globalPkgDir!,
        globalBinDir: opts.bin!,
        allowBuilds: opts.allowBuilds ?? {},
        params: [selector],
      }, commands)
    } catch (err: unknown) {
      everyPackageMigrated = false
      globalWarn(`Failed to migrate ${alias} from ${legacy.dir}: ${getSingleLineErrorMessage(err)}`)
    }
  }
  return everyPackageMigrated
}

/**
 * Delete the bins pnpm 10 linked into the pnpm home for the packages of
 * the previous project, then the project. A bin that cannot be removed
 * keeps the project, so the next `update -g` can retry.
 */
export async function removeLegacyGlobalLayout (legacy: LegacyGlobalLayout, pnpmHomeDir: string | undefined): Promise<void> {
  if (pnpmHomeDir != null && !await removeHomeBins(legacy, pnpmHomeDir)) {
    globalWarn(`Kept ${legacy.dir} because a bin pnpm 10 linked into the pnpm home could not be removed. ` +
      'The next "pnpm update -g" retries.')
    return
  }
  await fs.promises.rm(legacy.dir, { recursive: true, force: true })
  globalInfo(`Removed ${legacy.dir}`)
}

/** Returns whether every bin of the previous project was removed. */
async function removeHomeBins (legacy: LegacyGlobalLayout, pnpmHomeDir: string): Promise<boolean> {
  let files: string[]
  try {
    const bins = await getInstalledBins({ hash: '', installDir: legacy.dir, dependencies: legacy.dependencies })
    const candidates = await Promise.all(bins.map(async ({ name, path: target }) =>
      legacyBinFiles(path.join(pnpmHomeDir, name), legacy.dir, target)))
    files = [...new Set(candidates.flat())]
  } catch (err: unknown) {
    globalWarn(`Failed to read the bins linked from ${legacy.dir}: ${getSingleLineErrorMessage(err)}`)
    return false
  }
  const removed = await Promise.all(files.map(async (file) => {
    try {
      await fs.promises.rm(file, { force: true })
      return true
    } catch (err: unknown) {
      globalWarn(`Failed to remove ${file}: ${getSingleLineErrorMessage(err)}`)
      return false
    }
  }))
  return removed.every(Boolean)
}

/**
 * The files pnpm 10 wrote for the bin at `binPath` that are still a link
 * or shim into `legacyDir`, or a hard link to `target`. Each file is judged
 * on its own. Hard links are recognized only when the target resolves inside
 * `legacyDir`; other files at these paths are kept.
 */
export async function legacyBinFiles (binPath: string, legacyDir: string, target?: string): Promise<string[]> {
  const files = LEGACY_BIN_EXTENSIONS.map((extension) => `${binPath}${extension}`)
  const verdicts = await Promise.all(files.map((file) => isLegacyBin(file, legacyDir, target)))
  return files.filter((_, index) => verdicts[index])
}

export async function isLegacyBin (binPath: string, legacyDir: string, target?: string): Promise<boolean> {
  const stats = await getPathStats(binPath)
  if (!stats) return false
  const binDir = path.dirname(binPath)
  if (stats.isSymbolicLink()) {
    const linkTarget = path.resolve(binDir, await fs.promises.readlink(binPath))
    return pointsInto(linkTarget, legacyDir)
  }
  if (!stats.isFile()) return false
  if (target != null && await isHardLinkToTarget(stats, target, legacyDir)) {
    return true
  }
  if (stats.size > MAX_SHIM_BYTES) return false
  return shimTargetsDir(await fs.promises.readFile(binPath, 'utf8'), binDir, legacyDir)
}

async function getPathStats (binPath: string): Promise<fs.BigIntStats | null> {
  try {
    return await fs.promises.lstat(binPath, { bigint: true })
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return null
    throw err
  }
}

async function isHardLinkToTarget (
  stats: fs.BigIntStats,
  target: string,
  legacyDir: string
): Promise<boolean> {
  try {
    const [realTarget, realLegacyDir] = await Promise.all([
      fs.promises.realpath(target),
      fs.promises.realpath(legacyDir),
    ])
    if (!isSubdir(realLegacyDir, realTarget)) return false
    const targetStats = await fs.promises.stat(realTarget, { bigint: true })
    return stats.ino !== 0n && stats.ino === targetStats.ino && stats.dev === targetStats.dev
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
    return false
  }
}

/**
 * Whether `target` lies under `dir`, spelled as given or through the
 * symlinks of either: macOS spells a temp dir as `/var` and `/private/var`.
 */
async function pointsInto (target: string, dir: string): Promise<boolean> {
  if (isSubdir(dir, target)) return true
  return isSubdir(await realpathMissing(dir), await realpathMissing(target))
}

/**
 * Whether a shim in `shimDir` runs something under `dir`, which the shim
 * names either by its absolute path, in either spelling of a symlinked
 * directory, or relative to its own directory, with either kind of slash.
 */
async function shimTargetsDir (shimContent: string, shimDir: string, dir: string): Promise<boolean> {
  const content = shimContent.replaceAll('\\', '/')
  const prefixes = [dir, await realpathMissing(dir)]
  if (isSubdir(shimDir, dir)) {
    prefixes.push(path.relative(shimDir, dir))
  }
  return prefixes.some((prefix) => content.includes(`${prefix.replaceAll('\\', '/')}/node_modules/`))
}

/**
 * Resolve symlinks through the deepest existing ancestor of `targetPath`, then
 * re-append its missing tail. A path with no existing ancestor is returned
 * as it is.
 */
async function realpathMissing (targetPath: string): Promise<string> {
  const missing: string[] = []
  let current = targetPath
  for (;;) {
    try {
      // eslint-disable-next-line no-await-in-loop -- the walk climbs one ancestor at a time, so each step depends on the previous one
      return path.join(await fs.promises.realpath(current), ...missing.reverse())
    } catch (err: unknown) {
      if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
    }
    const parent = path.dirname(current)
    if (parent === current) return targetPath
    missing.push(path.basename(current))
    current = parent
  }
}
