import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'
import workerThreads from 'node:worker_threads'

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

/**
 * Writes buffer to a temp file then atomically renames it to the target path.
 * Used for recovery paths (overwriting corrupt/partial files) where in-place
 * writeFile would truncate the file and leave a window of invalid content.
 */
function writeFileAtomic (fileDest: string, buffer: Buffer, mode: number | undefined): void {
  const tempPath = `${fileDest}.${process.pid}-${workerThreads.threadId}.tmp`
  writeFile(tempPath, buffer, mode)
  fs.renameSync(tempPath, fileDest)
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
    // Use temp+rename so the replacement is atomic — no window where the
    // file is truncated or half-written.
    writeFileAtomic(fileDest, buffer, mode)
    const birthtimeMs = Date.now()
    locker.set(fileDest, birthtimeMs)
    return {
      checkedAt: birthtimeMs,
      filePath: fileDest,
    }
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
      writeFileAtomic(fileDest, buffer, mode)
      const birthtimeMs = Date.now()
      locker.set(fileDest, birthtimeMs)
      return {
        checkedAt: birthtimeMs,
        filePath: fileDest,
      }
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
