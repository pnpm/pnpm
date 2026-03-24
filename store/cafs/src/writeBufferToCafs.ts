import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

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
  } catch {
    return false
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
    // File exists but has wrong integrity (corruption) — overwrite it.
    writeFile(fileDest, buffer, mode)
    const birthtimeMs = Date.now()
    locker.set(fileDest, birthtimeMs)
    return {
      checkedAt: birthtimeMs,
      filePath: fileDest,
    }
  }

  // File doesn't exist. Use exclusive-create (O_CREAT|O_EXCL) for atomicity:
  // if another process creates the same CAS file concurrently, we get EEXIST
  // rather than a corrupted half-written file.
  try {
    writeFileExclusive(fileDest, buffer, mode)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') {
      // Another process created the same CAS file. Verify its integrity
      // before caching — the other process may have crashed mid-write.
      if (checkIntegrity(fileDest, integrity)) {
        const checkedAt = Date.now()
        locker.set(fileDest, checkedAt)
        return {
          checkedAt,
          filePath: fileDest,
        }
      }
      // Existing file has wrong integrity (partial write) — overwrite it.
      writeFile(fileDest, buffer, mode)
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
