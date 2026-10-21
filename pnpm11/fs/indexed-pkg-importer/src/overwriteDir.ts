import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { rimrafSync } from '@zkochan/rimraf'
import { fastPathTemp as pathTemp } from 'path-temp'

/**
 * Move `stage` onto `newDir`, replacing an existing entry at `newDir` by
 * moving it aside first instead of deleting it in place.
 *
 * `rename-overwrite` only moves an occupied target aside on `ENOTEMPTY`,
 * `EEXIST`, and `ENOTDIR`; on Windows `fs.renameSync` onto an existing
 * directory throws `EPERM` or `EACCES`, which `rename-overwrite` answers by
 * deleting the target in place and retrying in a 60-second loop.
 */
export function overwriteDirSync (stage: string, newDir: string): void {
  try {
    fs.renameSync(stage, newDir)
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err)) throw err
    switch (err.code) {
      case 'ENOENT':
        fs.statSync(stage)
        fs.mkdirSync(path.dirname(newDir), { recursive: true })
        overwriteDirSync(stage, newDir)
        return
      case 'EXDEV':
        copyStageAcrossDevicesSync(stage, newDir)
        return
      case 'ENOTEMPTY':
      case 'EEXIST':
      case 'ENOTDIR':
      case 'EPERM':
      case 'EACCES':
      case 'EBUSY':
        swapDirSync(stage, newDir)
        return
      default:
        throw err
    }
  }
}

function copyStageAcrossDevicesSync (stage: string, newDir: string): void {
  fs.mkdirSync(path.dirname(newDir), { recursive: true })
  const localStage = `${pathTemp(newDir)}_stage_${swapCounter++}`
  try {
    fs.cpSync(stage, localStage, { recursive: true, verbatimSymlinks: true })
    swapDirSync(localStage, newDir)
  } finally {
    removeQuietly(localStage)
  }
  removeQuietly(stage)
}

function swapDirSync (stage: string, newDir: string): void {
  const aside = moveExistingTargetAside(newDir)
  try {
    fs.renameSync(stage, newDir)
  } catch (err: unknown) {
    if (aside != null) {
      restoreOrRemoveAside(aside, newDir)
    }
    throw err
  }
  if (aside != null) {
    removeQuietly(aside)
  }
}

let swapCounter = 0

function moveExistingTargetAside (newDir: string): string | undefined {
  const aside = `${pathTemp(newDir)}_swap_${swapCounter++}`
  try {
    fs.renameSync(newDir, aside)
    return aside
  } catch (err: unknown) {
    return backupAndRemoveTargetAfterFailedDisplace(newDir, aside, err)
  }
}

function backupAndRemoveTargetAfterFailedDisplace (
  newDir: string,
  aside: string,
  err: unknown
): string | undefined {
  if (!isError(err) || !('code' in err)) throw err
  switch (err.code) {
    case 'ENOENT':
      return undefined
    // Windows refuses to rename a directory a process runs from; copy a
    // backup to `aside` before removing `newDir` in place so a failed
    // stage rename can still restore the previous package.
    case 'EPERM':
    case 'EACCES':
    case 'EBUSY':
      try {
        fs.cpSync(newDir, aside, { recursive: true, verbatimSymlinks: true })
        rimrafSync(newDir)
        return aside
      } catch (fallbackErr: unknown) {
        removeQuietly(aside)
        throw fallbackErr
      }
    default:
      throw err
  }
}

function restoreOrRemoveAside (aside: string, newDir: string): void {
  try {
    if (!fs.existsSync(newDir)) {
      fs.renameSync(aside, newDir)
      return
    }
  } catch {}
  if (fs.existsSync(newDir)) {
    removeQuietly(aside)
  }
}

function removeQuietly (target: string): void {
  try {
    rimrafSync(target)
  } catch {}
}
