import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'
import workerThreads from 'node:worker_threads'

import { renameOverwriteSync } from 'rename-overwrite'

import { type Integrity, verifyFileIntegrity } from './checkPkgFilesIntegrity.js'
import { writeFile, writeFileExclusive } from './writeFile.js'

export function writeBufferToCafs (
  locker: Map<string, number>,
  storeDir: string,
  buffer: Buffer,
  fileDest: string,
  mode: number | undefined,
  integrity: Integrity
): { checkedAt: number, filePath: string } {
  fileDest = path.join(storeDir, fileDest)
  if (locker.has(fileDest)) {
    return {
      checkedAt: locker.get(fileDest)!,
      filePath: fileDest,
    }
  }
  const checkedAt = writeOrCheck(fileDest, buffer, mode, integrity)
  locker.set(fileDest, checkedAt)
  return {
    checkedAt,
    filePath: fileDest,
  }
}

function writeOrCheck (
  fileDest: string,
  buffer: Buffer,
  mode: number | undefined,
  integrity: Integrity
): number {
  // Fast path: check if the file already exists on disk with correct content.
  const existingFile = fs.statSync(fileDest, { throwIfNoEntry: false })
  if (existingFile) {
    if (verifyFileIntegrity(fileDest, integrity)) {
      return Date.now()
    }
    // File exists but has wrong integrity (corruption/partial write).
    // Overwrite it in place when possible, keeping the inode so the
    // hard links to it from other projects' node_modules are healed by
    // the same write (pnpm/pnpm#3445). Fall back to atomic temp+rename
    // when in-place overwrite is refused or fails verification.
    if (overwriteFileInPlace(fileDest, buffer, integrity)) {
      return Date.now()
    }
    return writeFileAtomic(fileDest, buffer, mode)
  }

  // File doesn't exist. Use exclusive-create (O_CREAT|O_EXCL) so that
  // if another process creates the same CAS file concurrently, we get EEXIST
  // instead of silently overwriting. A crash mid-write can leave a partial
  // file, which is recovered by the atomic temp+rename path on next access.
  try {
    writeFileExclusive(fileDest, buffer, mode)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') {
      // Another process created the file. If it finished successfully,
      // integrity will pass. If it crashed or is still writing, integrity
      // will fail and we recover via atomic temp+rename.
      if (verifyFileIntegrity(fileDest, integrity)) {
        return Date.now()
      }
      return writeFileAtomic(fileDest, buffer, mode)
    }
    throw err
  }
  // Unfortunately, "birth time" (time of file creation) is available not on all filesystems.
  // We log the creation time ourselves and save it in the package index file.
  // Having this information allows us to skip content checks for files that were not modified since "birth time".
  return Date.now()
}

function writeFileAtomic (
  fileDest: string,
  buffer: Buffer,
  mode: number | undefined
): number {
  const temp = pathTemp(fileDest)
  writeFile(temp, buffer, mode)
  optimisticRenameOverwrite(temp, fileDest)
  return Date.now()
}

// O_NOFOLLOW keeps a symlink planted at the digest path from being
// followed into a file the store does not own, and O_NONBLOCK keeps a
// FIFO there from holding the open until a reader appears; both are
// no-ops for the regular files expected. Windows has neither flag;
// there the lstat check below stands alone.
const IN_PLACE_OPEN = fs.constants.O_WRONLY | fs.constants.O_TRUNC |
  (fs.constants.O_NOFOLLOW ?? 0) | (fs.constants.O_NONBLOCK ?? 0)

/**
 * Overwrites a corrupt store file in place — truncate and rewrite under
 * the same inode — so every hard link to it (in other projects'
 * node_modules) sees the restored content too. Returns false when the
 * repair should instead fall back to {@link writeFileAtomic}'s
 * temp+rename: the dirent is not a regular file, the file cannot be
 * opened for writing (read-only mode, ETXTBSY from a running
 * executable), the write failed, or the freshly written content does
 * not verify — the last covers a concurrent process still mid-write on
 * the same path, whose interleaved writes the rename then replaces.
 *
 * In-place overwrite is not atomic: a concurrent reader can observe
 * torn content for the duration of the write. The file was already
 * corrupt, and a failed read re-triggers verification and repair, so
 * this trades a brief torn-read window for healing every hard-linked
 * copy at once.
 */
function overwriteFileInPlace (
  fileDest: string,
  buffer: Buffer,
  integrity: Integrity
): boolean {
  const stats = fs.lstatSync(fileDest, { throwIfNoEntry: false })
  if (!stats?.isFile()) return false
  let fd: number
  try {
    fd = fs.openSync(fileDest, IN_PLACE_OPEN)
  } catch {
    return false
  }
  try {
    fs.writeFileSync(fd, buffer)
  } catch {
    return false
  } finally {
    try {
      fs.closeSync(fd)
    } catch {
      // Best-effort close; a close failure after a successful write is
      // caught by the verification below.
    }
  }
  return verifyFileIntegrity(fileDest, integrity)
}

export function optimisticRenameOverwrite (temp: string, fileDest: string): void {
  try {
    renameOverwriteSync(temp, fileDest)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') || !fs.existsSync(fileDest)) throw err
    // The temporary file path is created by appending the process ID to the target file name.
    // This is done to avoid lots of random crypto number generations.
    //   PR with related performance optimization: https://github.com/pnpm/pnpm/pull/6817
    //
    // Probably the only scenario in which the temp directory will disappear
    // before being renamed is when two containers use the same mounted directory
    // for their content-addressable store. In this case there's a chance that the process ID
    // will be the same in both containers.
    //
    // As a workaround, if the temp file doesn't exist but the target file does,
    // we just ignore the issue and assume that the target file is correct.
  }
}

/**
 * Creates a unique temporary file path by appending both process ID and worker thread ID
 * to the original filename.
 *
 * The process ID prevents conflicts between different processes, while the worker thread ID
 * prevents race conditions between threads in the same process.
 *
 * If a process fails, its temporary file may remain. When the process is rerun, it will
 * safely overwrite any existing temporary file with the same name.
 *
 * @param file - The original file path
 * @returns A temporary file path in the format: {basename}{pid}{threadId}
 */
function pathTemp (file: string): string {
  const basename = removeSuffix(path.basename(file))
  return path.join(path.dirname(file), `${basename}${process.pid}${workerThreads.threadId}`)
}

function removeSuffix (filePath: string): string {
  const dashPosition = filePath.indexOf('-')
  if (dashPosition === -1) return filePath
  const withoutSuffix = filePath.substring(0, dashPosition)
  if (filePath.substring(dashPosition) === '-exec') {
    return `${withoutSuffix}x`
  }
  return withoutSuffix
}
