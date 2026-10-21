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
        rimrafSync(newDir)
        fs.cpSync(stage, newDir, { recursive: true, verbatimSymlinks: true })
        rimrafSync(stage)
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
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return undefined
    }
    throw err
  }
}

function restoreOrRemoveAside (aside: string, newDir: string): void {
  try {
    if (!fs.existsSync(newDir)) {
      fs.renameSync(aside, newDir)
    }
  } catch {}
  removeQuietly(aside)
}

function removeQuietly (target: string): void {
  try {
    rimrafSync(target)
  } catch {}
}
