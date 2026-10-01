import fs from 'node:fs'
import path from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'

import { linkLogger } from '@pnpm/core-loggers'
import { isError } from '@pnpm/error'
import { isTransientFileLockError, withFileLockRetryAsync } from '@pnpm/fs.graceful-fs'
import { isSubdir } from 'is-subdir'
import { resolveLinkTarget } from 'resolve-link-target'
import { symlinkDir } from 'symlink-dir'

import { hasErrorCode, hoistLogger } from './hoistedModulesDirs.js'

export interface HoistLinkOwnerDirs {
  virtualStoreDir: string
  installStateDir: string
}

const MAX_LINK_READ_RETRIES = 100

export async function symlinkHoistedDependency (
  opts: HoistLinkOwnerDirs,
  depLocation: string,
  dest: string
): Promise<void> {
  return withFileLockRetryAsync(() => symlinkHoistedDependencyOnce(opts, depLocation, dest))
}

async function symlinkHoistedDependencyOnce (
  opts: HoistLinkOwnerDirs,
  depLocation: string,
  dest: string
): Promise<void> {
  if (await symlinkIfAbsent(depLocation, dest)) return
  let existingSymlink!: string
  try {
    existingSymlink = await withFileLockRetryAsync(() => resolveLinkTarget(dest))
  } catch (err: unknown) {
    return handleUnreadableExistingLink(err, depLocation, dest)
  }
  if (!isSubdir(opts.virtualStoreDir, existingSymlink) && !isSubdir(opts.installStateDir, existingSymlink)) {
    hoistLogger.debug({
      skipped: dest,
      existingSymlink,
      reason: 'an external symlink is present at the target location',
    })
    return
  }
  try {
    await fs.promises.unlink(dest)
  } catch (err: unknown) {
    if (!hasErrorCode(err, 'ENOENT')) throw err
  }
  await createHoistedDependencyLink(depLocation, dest)
}

async function symlinkIfAbsent (depLocation: string, dest: string): Promise<boolean> {
  try {
    await symlinkDir(depLocation, dest, { overwrite: false })
    linkLogger.debug({ target: dest, link: depLocation })
    return true
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'EEXIST' && err.code !== 'EISDIR') throw err
    return false
  }
}

async function handleUnreadableExistingLink (err: unknown, depLocation: string, dest: string): Promise<void> {
  if (hasErrorCode(err, 'ENOENT')) {
    return createHoistedDependencyLink(depLocation, dest)
  }
  if (!hasErrorCode(err, 'EINVAL')) throw err
  hoistLogger.debug({
    skipped: dest,
    reason: 'a directory is present at the target location',
  })
}

interface LinkRaceAttempt {
  depLocation: string
  dest: string
  linkReadRetries: number
}

async function createHoistedDependencyLink (depLocation: string, dest: string): Promise<void> {
  const attempt: LinkRaceAttempt = { depLocation, dest, linkReadRetries: 0 }
  let linked = false
  while (!linked) {
    // eslint-disable-next-line no-await-in-loop -- each attempt runs only after the previous one lost a race
    linked = await tryCreateHoistedDependencyLink(attempt)
  }
  linkLogger.debug({ target: dest, link: depLocation })
}

/**
 * Resolves to `true` once `dest` links to `depLocation`, whether this attempt or a
 * concurrent installer created it, and to `false` when the attempt should be retried.
 */
async function tryCreateHoistedDependencyLink (attempt: LinkRaceAttempt): Promise<boolean> {
  try {
    await symlinkDir(attempt.depLocation, attempt.dest, { overwrite: false })
    return true
  } catch (err: unknown) {
    if (!hasErrorCode(err, 'EEXIST') && !hasErrorCode(err, 'EISDIR')) throw err
    return acceptWinningLink(attempt, err)
  }
}

async function acceptWinningLink (attempt: LinkRaceAttempt, linkError: unknown): Promise<boolean> {
  let winningTarget: string
  try {
    winningTarget = await withFileLockRetryAsync(() => resolveLinkTarget(attempt.dest))
  } catch (readError: unknown) {
    attempt.linkReadRetries += 1
    if (await shouldRetryAfterLinkReadError(readError, attempt)) return false
    throw linkError
  }
  if (path.relative(attempt.depLocation, winningTarget) !== '') throw linkError
  return true
}

async function shouldRetryAfterLinkReadError (readError: unknown, attempt: LinkRaceAttempt): Promise<boolean> {
  if (!isError(readError) || !('code' in readError)) return false
  const canRetry = attempt.linkReadRetries <= MAX_LINK_READ_RETRIES
  if (readError.code === 'ENOENT' && canRetry) return true
  if (readError.code !== 'EINVAL') return false
  // macOS can report EINVAL when a concurrent unlink interrupts readlink.
  return shouldRetryInterruptedLinkRead(attempt.dest, canRetry)
}

async function shouldRetryInterruptedLinkRead (dest: string, canRetry: boolean): Promise<boolean> {
  try {
    const stat = await fs.promises.lstat(dest)
    if ((stat.isSymbolicLink() || await mayBeJunctionInCreation(dest, stat)) && canRetry) {
      await delay(1)
      return true
    }
  } catch (statError: unknown) {
    if (hasErrorCode(statError, 'ENOENT') && canRetry) return true
  }
  return false
}

/**
 * A junction is created as an empty directory that gets its reparse point
 * afterwards, so a concurrent hoist can find a plain directory in its place
 * for a moment. Until then its creator holds it open without sharing, so
 * listing it fails with a transient file-lock error, which counts as a
 * junction in creation. A junction completed after `stat` was read lists its
 * target's entries, so a non-empty directory is checked again. Always false
 * off Windows. Rejects with any other listing or inspection error.
 */
async function mayBeJunctionInCreation (dest: string, stat: fs.Stats): Promise<boolean> {
  if (process.platform !== 'win32' || !stat.isDirectory()) return false
  let entries: string[]
  try {
    entries = await fs.promises.readdir(dest)
  } catch (err: unknown) {
    if (isTransientFileLockError(err)) return true
    throw err
  }
  if (entries.length === 0) return true
  return (await fs.promises.lstat(dest)).isSymbolicLink()
}
