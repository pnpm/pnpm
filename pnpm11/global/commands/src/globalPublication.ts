import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { linkBinsOfPackages } from '@pnpm/bins.linker'
import { removeBin } from '@pnpm/bins.remover'
import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import { PnpmError } from '@pnpm/error'
import { getHashLink, getInstalledBinNames, type GlobalPackageInfo } from '@pnpm/global.packages'
import type { DependencyManifest } from '@pnpm/types'
import { isSubdir } from 'is-subdir'
import { symlinkDir } from 'symlink-dir'

export interface PublishGlobalInstallOptions {
  installDir: string
  hashLink: string
  globalBinDir: string
  pkgs: Array<{ manifest: DependencyManifest, location: string }>
  binsToSkip: Set<string>
}

export interface CleanupReplacedGlobalInstallsOptions {
  groups: GlobalPackageInfo[]
  globalDir: string
  globalBinDir: string
  activeHash: string
  publishedBins: Set<string>
  protectedBins: Set<string>
}

interface SavedBinSlot {
  original: string
  backup: string
}

interface PreparedGlobalInstall {
  actualBinNames: Set<string>
  backupDir: string
  savedBinSlots: SavedBinSlot[]
  oldHashTarget: string | undefined
}

export async function publishGlobalInstall (
  opts: PublishGlobalInstallOptions
): Promise<Set<string>> {
  const prepared = await prepareGlobalInstall(opts)
  try {
    for (const name of prepared.actualBinNames) {
      // eslint-disable-next-line no-await-in-loop -- Publication mutations must settle before linking or rollback starts.
      await removeBin(path.join(opts.globalBinDir, name))
    }
    await linkBinsOfPackages(opts.pkgs, opts.globalBinDir, { excludeBins: opts.binsToSkip })
    await symlinkDir(opts.installDir, opts.hashLink, { overwrite: true })
  } catch (publicationError) {
    try {
      await restoreGlobalInstall({ ...opts, ...prepared })
    } catch (rollbackError) {
      const rollbackMessage = getErrorMessage(rollbackError)
      throw new PnpmError(
        'GLOBAL_BIN_ROLLBACK_FAILED',
        'Failed to restore global bins after publication failed. ' +
          `Recovery files remain at ${prepared.backupDir}; the fresh install remains at ${opts.installDir}. ` +
          `Rollback error: ${rollbackMessage}`,
        { cause: publicationError }
      )
    }
    await cleanupFailedGlobalPublication({ ...opts, ...prepared }, publicationError)
    throw publicationError
  }
  // Publication is already committed, so a leftover backup directory must
  // not fail the command.
  try {
    await fs.promises.rm(prepared.backupDir, { recursive: true, force: true })
  } catch {}
  return prepared.actualBinNames
}

export async function cleanupReplacedGlobalInstalls (
  opts: CleanupReplacedGlobalInstallsOptions
): Promise<void> {
  const errors: unknown[] = []
  for (const group of opts.groups) {
    // eslint-disable-next-line no-await-in-loop -- Cleanup mutations must settle before the next group starts.
    errors.push(...await cleanupReplacedGlobalInstall(opts, group))
  }
  if (errors.length === 1) throw errors[0]
  if (errors.length > 1) {
    throw new AggregateError(errors, 'Failed to clean up replaced global installs')
  }
}

// Publication already succeeded when this runs, so every removal is
// attempted even after one fails; the failures are aggregated instead of
// aborting the remaining cleanup.
async function cleanupReplacedGlobalInstall (
  opts: CleanupReplacedGlobalInstallsOptions,
  group: GlobalPackageInfo
): Promise<unknown[]> {
  const errors: unknown[] = []
  let binNames: string[]
  try {
    binNames = await getInstalledBinNames(group)
  } catch (err) {
    // The install directory is the only record of which bins the group
    // owns, so removing it now would strand them on PATH forever. Leave
    // the group intact for a later run to clean up.
    return [err]
  }
  for (const binName of binNames) {
    if (opts.publishedBins.has(binName) || opts.protectedBins.has(binName)) continue
    try {
      await removeBin(path.join(opts.globalBinDir, binName)) // eslint-disable-line no-await-in-loop -- Each removal must settle before cleanup continues.
    } catch (err) {
      errors.push(err)
    }
  }
  if (group.hash !== opts.activeHash) {
    try {
      await fs.promises.rm(getHashLink(opts.globalDir, group.hash), { force: true })
    } catch (err) {
      errors.push(err)
    }
  }
  if (isSubdir(opts.globalDir, group.installDir)) {
    try {
      await fs.promises.rm(group.installDir, { recursive: true, force: true })
    } catch (err) {
      errors.push(err)
    }
  }
  return errors
}

async function prepareGlobalInstall (
  opts: PublishGlobalInstallOptions
): Promise<PreparedGlobalInstall> {
  let backupDir: string | undefined
  try {
    const actualBinNames = await getActualBinNames(opts)
    // The backup directory lives in the global bin directory, which the
    // linker would otherwise be the first to create.
    await fs.promises.mkdir(opts.globalBinDir, { recursive: true })
    backupDir = await fs.promises.mkdtemp(path.join(opts.globalBinDir, '.pnpm-bin-backup-'))
    const savedBinSlots = await backupBinSlots({
      actualBinNames,
      backupDir,
      globalBinDir: opts.globalBinDir,
    })
    const oldHashTarget = await readHashTarget(opts.hashLink)
    return { actualBinNames, backupDir, savedBinSlots, oldHashTarget }
  } catch (preparationError) {
    const cleanupResults = await Promise.allSettled([
      ...(backupDir == null ? [] : [fs.promises.rm(backupDir, { recursive: true, force: true })]),
      fs.promises.rm(opts.installDir, { recursive: true, force: true }),
    ])
    const cleanupErrors = cleanupResults.flatMap((result) => {
      return result.status === 'rejected' ? [result.reason] : []
    })
    if (cleanupErrors.length > 0) {
      throw new AggregateError(
        [preparationError, ...cleanupErrors],
        'Failed to clean up after global bin publication preparation failed.',
        { cause: preparationError }
      )
    }
    throw preparationError
  }
}

async function getActualBinNames (opts: PublishGlobalInstallOptions): Promise<Set<string>> {
  const actualBinNames = new Set<string>()
  const binsByPackage = await Promise.all(opts.pkgs.map(async ({ manifest, location }) => {
    return getBinsFromPackageManifest(manifest, location)
  }))
  for (const bins of binsByPackage) {
    for (const { name } of bins) {
      if (!opts.binsToSkip.has(name)) actualBinNames.add(name)
    }
  }
  return actualBinNames
}

async function backupBinSlots (opts: {
  actualBinNames: Set<string>
  backupDir: string
  globalBinDir: string
}): Promise<SavedBinSlot[]> {
  const extensions = process.platform === 'win32' ? ['', '.cmd', '.ps1', '.exe'] : ['']
  const originals: string[] = []
  const seenOriginals = new Set<string>()
  for (const name of opts.actualBinNames) {
    for (const extension of extensions) {
      const original = path.join(opts.globalBinDir, `${name}${extension}`)
      if (!seenOriginals.has(original)) {
        seenOriginals.add(original)
        originals.push(original)
      }
    }
  }
  const savedBinSlots: SavedBinSlot[] = []
  for (const [index, original] of originals.entries()) {
    const savedBinSlot = await backupBinSlot({ // eslint-disable-line no-await-in-loop
      original,
      backup: path.join(opts.backupDir, String(index)),
    })
    if (savedBinSlot != null) savedBinSlots.push(savedBinSlot)
  }
  return savedBinSlots
}

async function backupBinSlot (slot: SavedBinSlot): Promise<SavedBinSlot | undefined> {
  let stat: fs.Stats
  try {
    stat = await fs.promises.lstat(slot.original)
  } catch (err) {
    if (isErrorWithCode(err, 'ENOENT')) return undefined
    throw err
  }
  if (stat.isSymbolicLink()) {
    const type = await getSymlinkType(slot.original)
    await fs.promises.symlink(await fs.promises.readlink(slot.original), slot.backup, type)
    return slot
  }
  if (stat.isFile()) {
    await fs.promises.copyFile(slot.original, slot.backup, fs.constants.COPYFILE_FICLONE)
    await fs.promises.chmod(slot.backup, stat.mode)
    return slot
  }
  throw new PnpmError(
    'GLOBAL_BIN_UNSUPPORTED_TYPE',
    `Cannot replace global bin slot at ${slot.original}: expected a regular file or symbolic link`
  )
}

async function getSymlinkType (link: string): Promise<fs.symlink.Type | undefined> {
  if (process.platform !== 'win32') return undefined
  try {
    return (await fs.promises.stat(link)).isDirectory() ? 'dir' : 'file'
  } catch (err) {
    // A dangling bin link has no resolvable target; back it up as a file link.
    if (isErrorWithCode(err, 'ENOENT')) return 'file'
    throw err
  }
}

async function readHashTarget (hashLink: string): Promise<string | undefined> {
  try {
    return await fs.promises.realpath(hashLink)
  } catch (err) {
    if (isErrorWithCode(err, 'ENOENT')) return undefined
    throw err
  }
}

async function restoreGlobalInstall (opts: PublishGlobalInstallOptions & PreparedGlobalInstall): Promise<void> {
  for (const name of opts.actualBinNames) {
    // eslint-disable-next-line no-await-in-loop -- Rollback mutation must settle before the next step or return.
    await removeBin(path.join(opts.globalBinDir, name))
  }
  for (const { original, backup } of opts.savedBinSlots) {
    // eslint-disable-next-line no-await-in-loop -- Rollback mutation must settle before the next step or return.
    await fs.promises.rename(backup, original)
  }
  if (opts.oldHashTarget == null) {
    await fs.promises.rm(opts.hashLink, { force: true, recursive: true })
  } else {
    // symlink-dir creates a relative link, so both paths must use the same canonical parent space.
    const canonicalHashLink = path.join(
      await fs.promises.realpath(path.dirname(opts.hashLink)),
      path.basename(opts.hashLink)
    )
    await symlinkDir(opts.oldHashTarget, canonicalHashLink, { overwrite: true })
  }
}

async function cleanupFailedGlobalPublication (
  opts: PublishGlobalInstallOptions & PreparedGlobalInstall,
  publicationError: unknown
): Promise<void> {
  const cleanupResults = await Promise.allSettled([
    fs.promises.rmdir(opts.backupDir),
    fs.promises.rm(opts.installDir, { recursive: true, force: true }),
  ])
  const cleanupErrors = cleanupResults.flatMap((result) => {
    return result.status === 'rejected' ? [result.reason] : []
  })
  if (cleanupErrors.length === 0) return

  const artifactPaths = [opts.backupDir, opts.installDir]
  const artifactResults = await Promise.allSettled(artifactPaths.map(pathExists))
  const remainingPaths = artifactResults.flatMap((result, index) => {
    if (result.status === 'rejected') {
      cleanupErrors.push(result.reason)
      return []
    }
    return result.value ? [artifactPaths[index]] : []
  })
  const remainingMessage = remainingPaths.length === 0
    ? ''
    : ` Remaining artifacts: ${remainingPaths.join(', ')}.`
  throw new AggregateError(
    [publicationError, ...cleanupErrors],
    `Failed to clean up after global bin publication failed.${remainingMessage}`,
    { cause: publicationError }
  )
}

async function pathExists (target: string): Promise<boolean> {
  try {
    await fs.promises.lstat(target)
    return true
  } catch (err) {
    if (isErrorWithCode(err, 'ENOENT')) return false
    throw err
  }
}

function isErrorWithCode (err: unknown, code: string): boolean {
  return util.types.isNativeError(err) && 'code' in err && err.code === code
}

function getErrorMessage (err: unknown): string {
  if (util.types.isNativeError(err)) return err.message
  try {
    return String(err)
  } catch {
    return 'Unknown error'
  }
}
