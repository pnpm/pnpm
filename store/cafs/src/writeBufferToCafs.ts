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
    // Use temp+rename so the replacement is atomic.
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

function optimisticRenameOverwrite (temp: string, fileDest: string): void {
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
