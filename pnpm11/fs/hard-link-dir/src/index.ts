import assert from 'node:assert'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import gfs, { renameFileWithRetry } from '@pnpm/fs.graceful-fs'
import { globalWarn } from '@pnpm/logger'
import { fastPathTemp } from 'path-temp'

/**
 * Hard links the contents of `src` into every directory of `destDirs`, leaving
 * out `node_modules`. A destination that is `src` itself is skipped; the rest
 * are created if they are missing, and a missing `src` leaves them empty and
 * warns.
 *
 * A destination is filled in place, entry by entry. Its own `node_modules` —
 * under the hoisted node linker, the dependencies that could not be hoisted any
 * higher — stays where it is, and so does anyone working inside it: the copies
 * of one build chunk run concurrently, so a sibling package may be staging its
 * own copy in there while this one is written. Entries the destination has that
 * `src` does not are left alone for the same reason.
 *
 * A destination entry of the kind `src` does not have is removed to make room,
 * a file already there is replaced through a rename, and a filesystem error
 * that neither of those resolves is thrown.
 */
export function hardLinkDir (src: string, destDirs: string[]): void {
  const targetDirs = destDirs.filter((destDir) => path.relative(destDir, src) !== '')
  if (targetDirs.length === 0) return
  for (const targetDir of targetDirs) {
    gfs.mkdirSync(targetDir, { recursive: true })
  }
  _hardLinkDir(src, targetDirs, true)
}

function _hardLinkDir (src: string, destDirs: string[], isRoot?: boolean): void {
  let files: string[] = []
  try {
    files = fs.readdirSync(src)
  } catch (err: unknown) {
    if (!isRoot || !((util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT'))) throw err
    globalWarn(`Source directory not found when creating hardLinks for: ${src}. Creating destinations as empty: ${destDirs.join(', ')}`)
    return
  }
  for (const file of files) {
    if (file === 'node_modules') continue
    const srcFile = path.join(src, file)
    const srcStats = fs.lstatSync(srcFile, { bigint: true })
    if (srcStats.isDirectory()) {
      const destSubdirs = destDirs.map((destDir) => {
        const destSubdir = path.join(destDir, file)
        clearMismatchedDirent(destSubdir, true)
        try {
          gfs.mkdirSync(destSubdir, { recursive: true })
        } catch (err: unknown) {
          if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST')) throw err
        }
        return destSubdir
      })
      _hardLinkDir(srcFile, destSubdirs)
      continue
    }
    for (const destDir of destDirs) {
      const destFile = path.join(destDir, file)
      try {
        linkOrCopyFile(srcFile, destFile, srcStats)
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
          // Ignore broken symlinks
          continue
        }
        throw err
      }
    }
  }
}

function linkOrCopyFile (srcFile: string, destFile: string, srcStats: fs.BigIntStats): void {
  try {
    linkOrCopy(srcFile, destFile)
    return
  } catch (err: unknown) {
    assert(util.types.isNativeError(err))
    if ('code' in err && err.code === 'ENOENT') {
      gfs.mkdirSync(path.dirname(destFile), { recursive: true })
      linkOrCopy(srcFile, destFile)
      return
    }
    if (!('code' in err && err.code === 'EEXIST')) {
      throw err
    }
  }
  // Most of what a destination holds is the very file the source holds: both
  // were linked from the same store entry, and a build touches only a few of
  // them. Replacing those again would cost a link and a rename apiece.
  if (isSameFile(destFile, srcStats)) return
  replaceFile(srcFile, destFile)
}

// Read as bigints: a 64-bit inode does not survive a JavaScript number, so two
// unrelated files can round-trip to the same one.
function isSameFile (destFile: string, srcStats: fs.BigIntStats): boolean {
  let destStats
  try {
    destStats = fs.lstatSync(destFile, { bigint: true })
  } catch (err: unknown) {
    // The caller got EEXIST for this path, so only a concurrent removal
    // explains it being gone; anything else is a real failure to report.
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
  // Filesystems that report neither an inode nor a device (some on Windows)
  // make every file look like every other one.
  return destStats.ino !== 0n &&
    destStats.dev !== 0n &&
    destStats.ino === srcStats.ino &&
    destStats.dev === srcStats.dev
}

// Swap the new file in through a temp sibling, so that whoever reads the
// destination sees either the whole old file or the whole new one.
function replaceFile (srcFile: string, destFile: string): void {
  const tempFile = fastPathTemp(destFile)
  // A temp file is named after the thread that writes it, so only an
  // interrupted run can leave one behind for this one to trip over.
  fs.rmSync(tempFile, { recursive: true, force: true })
  try {
    linkOrCopy(srcFile, tempFile)
    clearMismatchedDirent(destFile, false)
    renameFileWithRetry(tempFile, destFile)
  } catch (err: unknown) {
    try {
      fs.unlinkSync(tempFile)
    } catch {} // eslint-disable-line:no-empty
    throw err
  }
}

// A directory cannot be created where a file or a symlink is, and a rename
// cannot put a file where a directory is. The destination holds an older copy
// of the same package, so only a build that turned one into the other leaves it
// with a dirent of the wrong kind. Removing it also keeps a symlinked directory
// from redirecting the writes below out of the destination.
function clearMismatchedDirent (destPath: string, wantDirectory: boolean): void {
  let stats
  try {
    stats = fs.lstatSync(destPath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
  if (stats.isDirectory() === wantDirectory) return
  fs.rmSync(destPath, { recursive: true, force: true })
}

/*
 * This function could be optimized because we don't really need to try linking again
 * if linking failed once.
 */
function linkOrCopy (srcFile: string, destFile: string): void {
  try {
    gfs.linkSync(srcFile, destFile)
  } catch (err: unknown) {
    // In some container environments (OverlayFS), linkSync throws ENOENT
    // instead of EXDEV when linking across layers. We must fallback to copy in this case too.
    if (util.types.isNativeError(err) && 'code' in err && (err.code === 'EXDEV' || err.code === 'ENOENT')) {
      gfs.copyFileSync(srcFile, destFile)
    } else {
      throw err
    }
  }
}
