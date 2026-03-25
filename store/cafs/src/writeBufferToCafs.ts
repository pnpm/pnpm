import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'
import workerThreads from 'node:worker_threads'

import { renameOverwriteSync } from 'rename-overwrite'

import type { Integrity } from './checkPkgFilesIntegrity.js'
import { writeFile, writeFileExclusive } from './writeFile.js'

/**
 * Non-destructive integrity check: reads the file and compares its hash
 * against the expected digest. Unlike verifyFileIntegrity(), this does NOT
 * delete the file on mismatch — which is important when another process may
 * still be writing to the same CAS path.
 */
function checkIntegrity (filename: string, integrity: Integrity): boolean {
  let data: Buffer
  try {
    data = fs.readFileSync(filename)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return false
    }
    throw err
  }
  try {
    return crypto.hash(integrity.algorithm, data, 'hex') === integrity.digest
  } catch {
    return false
  }
}

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
  // Fast path: check if the file already exists on disk with correct content.
  const existingFile = fs.statSync(fileDest, { throwIfNoEntry: false })
  if (existingFile) {
    if (checkIntegrity(fileDest, integrity)) {
      const checkedAt = Date.now()
      locker.set(fileDest, checkedAt)
      return {
        checkedAt,
        filePath: fileDest,
      }
    }
    // File exists but has wrong integrity (corruption/partial write).
    // Use temp+rename so the replacement is atomic.
    return writeViaTempFile(locker, fileDest, buffer, mode)
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
      if (checkIntegrity(fileDest, integrity)) {
        const checkedAt = Date.now()
        locker.set(fileDest, checkedAt)
        return {
          checkedAt,
          filePath: fileDest,
        }
      }
      return writeViaTempFile(locker, fileDest, buffer, mode)
    }
    throw err
  }
  const birthtimeMs = Date.now()
  locker.set(fileDest, birthtimeMs)
  return {
    checkedAt: birthtimeMs,
    filePath: fileDest,
  }
}

function writeViaTempFile (
  locker: Map<string, number>,
  fileDest: string,
  buffer: Buffer,
  mode: number | undefined
): { checkedAt: number, filePath: string } {
  const temp = pathTemp(fileDest)
  writeFile(temp, buffer, mode)
  const birthtimeMs = Date.now()
  renameOverwriteSync(temp, fileDest)
  locker.set(fileDest, birthtimeMs)
  return {
    checkedAt: birthtimeMs,
    filePath: fileDest,
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
