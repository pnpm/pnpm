import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { getBinsToLink, linkBinsOfPackages } from '@pnpm/bins.linker'
import { removeBin } from '@pnpm/bins.remover'
import { PnpmError } from '@pnpm/error'
import { getHashLink, type GlobalPackageBinSnapshot } from '@pnpm/global.packages'
import { globalWarn } from '@pnpm/logger'
import type { DependencyManifest } from '@pnpm/types'
import { isSubdir } from 'is-subdir'
import { symlinkDir } from 'symlink-dir'

import { getErrorMessage } from './errorMessage.js'

export interface ActivateGlobalInstallOptions {
  installDir: string
  hashLink: string
  globalBinDir: string
  pkgs: Array<{ manifest: DependencyManifest, location: string }>
  binsToSkip: Set<string>
  requiredBinNames?: Set<string>
}

export interface CleanupReplacedGlobalInstallsOptions {
  groups: GlobalPackageBinSnapshot[]
  globalDir: string
  globalBinDir: string
  activeHash: string
  activatedBins: Set<string>
  protectedBins: Set<string>
}

interface SavedBinSlot {
  original: string
  backup: string
}

interface PreparedGlobalInstall {
  actualBins: Map<string, string>
  actualBinNames: Set<string>
  backupDir: string
  savedBinSlots: SavedBinSlot[]
  oldHashTarget: string | undefined
}

export async function activateGlobalInstall (
  opts: ActivateGlobalInstallOptions
): Promise<Set<string>> {
  const prepared = await prepareGlobalInstall(opts)
  try {
    // Moving the hash link is the switch-over: the shims resolve
    // through it, so every command the group already provides starts
    // running the new install here, in one step. Linking afterwards only
    // has to write the shims whose target actually changed, which for an
    // update of the same commands is none of them.
    await swapHashLink(opts.installDir, opts.hashLink)
    await linkBinsOfPackages(hashLinkedPkgs(opts), opts.globalBinDir, { excludeBins: opts.binsToSkip })
    await ensureRequiredBinTargets(opts.requiredBinNames, prepared.actualBins)
    await removeSlotsOfMissingBins(opts, prepared.actualBins)
  } catch (activationError) {
    try {
      await restoreGlobalInstall({ ...opts, ...prepared })
    } catch (rollbackError) {
      const rollbackMessage = getErrorMessage(rollbackError)
      throw new PnpmError(
        'GLOBAL_BIN_ROLLBACK_FAILED',
        'Failed to restore global bins after activation failed. ' +
          `Recovery files remain at ${prepared.backupDir}; the fresh install remains at ${opts.installDir}. ` +
          `Rollback error: ${rollbackMessage}`,
        { cause: activationError }
      )
    }
    await cleanupFailedGlobalActivation({ ...opts, ...prepared }, activationError)
    throw activationError
  }
  try {
    await fs.promises.rm(prepared.backupDir, { recursive: true, force: true })
  } catch (err) {
    // Activation is already committed, so a leftover backup directory
    // must not fail the command — but it points at a filesystem problem
    // worth surfacing.
    globalWarn(`Failed to remove the global bin backup directory at ${prepared.backupDir}: ${getErrorMessage(err)}`)
  }
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

// Activation already succeeded when this runs, so every removal is
// attempted even after one fails; the failures are aggregated instead of
// aborting the remaining cleanup.
async function cleanupReplacedGlobalInstall (
  opts: CleanupReplacedGlobalInstallsOptions,
  groupSnapshot: GlobalPackageBinSnapshot
): Promise<unknown[]> {
  const errors: unknown[] = []
  const { info: group, binNames } = groupSnapshot
  let binRemovalFailed = false
  for (const binName of binNames) {
    if (opts.activatedBins.has(binName) || opts.protectedBins.has(binName)) continue
    try {
      await removeBin(path.join(opts.globalBinDir, binName)) // eslint-disable-line no-await-in-loop -- Each removal must settle before cleanup continues.
    } catch (err) {
      errors.push(err)
      binRemovalFailed = true
    }
  }
  // The group's install directory is what a later run reads its ownership
  // from, so keep the group until every one of its bins is gone.
  if (binRemovalFailed) return errors
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

/**
 * The packages to link from, addressed through the group's hash link
 * instead of the generation directory it currently points at. Bin shims
 * embed the path they are generated from, so this is what makes a shim
 * survive the next update untouched.
 */
function hashLinkedPkgs (opts: ActivateGlobalInstallOptions): ActivateGlobalInstallOptions['pkgs'] {
  return opts.pkgs.map((pkg) => {
    if (!isSubdir(opts.installDir, pkg.location)) return pkg
    return { ...pkg, location: path.join(opts.hashLink, path.relative(opts.installDir, pkg.location)) }
  })
}

/**
 * Point `hashLink` at `target`, replacing any existing link in a single
 * step so a concurrent command never observes it missing. Windows cannot
 * rename over an existing junction, so there the replacement is not
 * atomic.
 */
async function swapHashLink (target: string, hashLink: string): Promise<void> {
  if (process.platform === 'win32') {
    await symlinkDir(target, hashLink, { overwrite: true })
    return
  }
  await fs.promises.mkdir(path.dirname(hashLink), { recursive: true })
  const [linkParent, linkTarget] = await Promise.all([
    fs.promises.realpath(path.dirname(hashLink)),
    fs.promises.realpath(target),
  ])
  const stagedLink = `${hashLink}.${process.pid}.tmp`
  await fs.promises.rm(stagedLink, { force: true, recursive: true })
  await fs.promises.symlink(path.relative(linkParent, linkTarget), stagedLink, 'dir')
  try {
    await fs.promises.rename(stagedLink, hashLink)
  } catch (err) {
    await fs.promises.rm(stagedLink, { force: true, recursive: true })
    throw err
  }
}

async function prepareGlobalInstall (
  opts: ActivateGlobalInstallOptions
): Promise<PreparedGlobalInstall> {
  let backupDir: string | undefined
  try {
    const actualBins = await getActualBins(opts)
    const actualBinNames = new Set(actualBins.keys())
    ensureRequiredBinNames(opts.requiredBinNames, actualBinNames)
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
    return { actualBins, actualBinNames, backupDir, savedBinSlots, oldHashTarget }
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
        'Failed to clean up after global bin activation preparation failed.',
        { cause: preparationError }
      )
    }
    throw preparationError
  }
}

/**
 * Resolves commands not skipped whose target files exist. Missing targets are
 * omitted, while package-manifest resolution and filesystem errors propagate.
 */
async function getActualBins (
  opts: Pick<ActivateGlobalInstallOptions, 'pkgs' | 'binsToSkip'>
): Promise<Map<string, string>> {
  const actualBins = new Map<string, string>()
  const bins = await getBinsToLink(opts.pkgs, opts.binsToSkip)
  const existing = await Promise.all(bins.map(async ({ path: binPath }) => pathExists(binPath)))
  for (const [index, { name, path: binPath }] of bins.entries()) {
    if (existing[index]) actualBins.set(name, binPath)
  }
  return actualBins
}

export async function getActualBinNames (
  opts: Pick<ActivateGlobalInstallOptions, 'pkgs' | 'binsToSkip'>
): Promise<Set<string>> {
  return new Set((await getActualBins(opts)).keys())
}

async function ensureRequiredBinTargets (
  required: Set<string> | undefined,
  actualBins: Map<string, string>
): Promise<void> {
  const requiredBins = [...required ?? []].map((name) => [name, actualBins.get(name)] as const)
  const existing = await Promise.all(requiredBins.map(async ([, binPath]) => binPath != null && pathExists(binPath)))
  const missing = requiredBins
    .filter(([, binPath], index) => binPath == null || !existing[index])
    .map(([name]) => name)
    .sort()
  if (missing.length > 0) throwMissingBinTargets(missing)
}

function ensureRequiredBinNames (required: Set<string> | undefined, actual: Set<string>): void {
  const missing = [...required ?? []].filter((name) => !actual.has(name)).sort()
  if (missing.length > 0) throwMissingBinTargets(missing)
}

function throwMissingBinTargets (missing: string[]): never {
  throw new PnpmError(
    'GLOBAL_BIN_TARGET_MISSING',
    `Global bin targets disappeared during activation: ${missing.join(', ')}`
  )
}

/**
 * Drop the slots of commands whose target disappeared during activation.
 */
async function removeSlotsOfMissingBins (
  opts: ActivateGlobalInstallOptions,
  actualBins: Map<string, string>
): Promise<void> {
  const missing = (await Promise.all([...actualBins].map(async ([name, binPath]) => {
    return await pathExists(binPath) ? [] : [name]
  }))).flat()
  for (const name of missing) {
    await removeBin(path.join(opts.globalBinDir, name)) // eslint-disable-line no-await-in-loop -- Each removal must settle before the next.
  }
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

async function restoreGlobalInstall (opts: ActivateGlobalInstallOptions & PreparedGlobalInstall): Promise<void> {
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
    await swapHashLink(opts.oldHashTarget, opts.hashLink)
  }
}

async function cleanupFailedGlobalActivation (
  opts: ActivateGlobalInstallOptions & PreparedGlobalInstall,
  activationError: unknown
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
    [activationError, ...cleanupErrors],
    `Failed to clean up after global bin activation failed.${remainingMessage}`,
    { cause: activationError }
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
