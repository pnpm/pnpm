import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'
import workerThreads from 'node:worker_threads'

import { renameOverwriteSync } from 'rename-overwrite'

import { writeFile, writeFileExclusive } from './writeFile.js'

export function writeBufferToCafs (
  locker: Map<string, number>,
  storeDir: string,
  buffer: Buffer,
  fileDest: string,
  mode: number | undefined
): { checkedAt: number, filePath: string } {
  fileDest = path.join(storeDir, fileDest)
  if (locker.has(fileDest)) {
    return {
      checkedAt: locker.get(fileDest)!,
      filePath: fileDest,
    }
  }
  const checkedAt = writeOrCheck(fileDest, buffer, mode)
  locker.set(fileDest, checkedAt)
  return {
    checkedAt,
    filePath: fileDest,
  }
}

function writeOrCheck (
  fileDest: string,
  buffer: Buffer,
  mode: number | undefined
): number {
  // Fast path: check if the file already exists on disk with correct size.
  // In a content-addressable store, the file path is derived from the content hash.
  // If a file exists at this path with the expected size, it is almost certainly
  // the correct content. The full integrity verification in checkPkgFilesIntegrity
  // will catch any corruption on the next install.
  const existingFile = fs.statSync(fileDest, { throwIfNoEntry: false })
  if (existingFile) {
    if (existingFile.size === buffer.length) {
      return Date.now()
    }
    // File exists but has wrong size (corruption/partial write).
    // Use temp+rename so the replacement is atomic.
    return writeFileAtomic(fileDest, buffer, mode)
  }

  // File doesn't exist. Use exclusive-create (O_CREAT|O_EXCL) so that
  // if another process creates the same CAS file concurrently, we get EEXIST
  // instead of silently overwriting.
  try {
    writeFileExclusive(fileDest, buffer, mode)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') {
      // Another process created the file. Check if it's complete.
      const stat = fs.statSync(fileDest, { throwIfNoEntry: false })
      if (stat && stat.size === buffer.length) {
        return Date.now()
      }
      // File exists but incomplete or corrupted. Overwrite atomically.
      return writeFileAtomic(fileDest, buffer, mode)
    }
    throw err
  }
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
    // for their content-addressable store. In this case there is a chance that the process ID
    // will be the same in both containers.
    //
    // As a workaround, if the temp file does not exist but the target file does,
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
