import { promises as fs, type Stats } from 'node:fs'
import path from 'node:path'

import fixBin from 'bin-links/lib/fix-bin.js'

// Reports whether two paths refer to the same file. A matching inode/device
// pair (read as BigInts to avoid the precision loss of NTFS 64-bit file IDs)
// proves a hard link cheaply. Whenever identity can't be established that way —
// because the inodes genuinely differ or because Windows reports an unreliable
// zero inode — we fall back to comparing the file contents after a quick size
// check, which also treats a byte-identical copy as the same file.
export async function isSameFile (pathA: string, pathB: string): Promise<boolean> {
  const [statA, statB] = await Promise.all([
    fs.stat(pathA, { bigint: true }).catch(() => null),
    fs.stat(pathB, { bigint: true }).catch(() => null),
  ])
  if (statA == null || statB == null) return false
  if (statA.ino && statB.ino && statA.ino === statB.ino && statA.dev === statB.dev) {
    return true
  }
  if (statA.size !== statB.size) return false
  return haveEqualContents(pathA, pathB)
}

const FILE_COMPARE_CHUNK_SIZE = 64 * 1024

// Compares two equally-sized files chunk by chunk, so an executable is never
// fully buffered in memory and a mismatch returns as early as possible.
async function haveEqualContents (pathA: string, pathB: string): Promise<boolean> {
  const [fhA, fhB] = await Promise.all([
    fs.open(pathA, 'r').catch(() => null),
    fs.open(pathB, 'r').catch(() => null),
  ])
  if (fhA == null || fhB == null) {
    await fhA?.close().catch(() => {})
    await fhB?.close().catch(() => {})
    return false
  }
  try {
    const bufA = Buffer.alloc(FILE_COMPARE_CHUNK_SIZE)
    const bufB = Buffer.alloc(FILE_COMPARE_CHUNK_SIZE)
    let position = 0
    for (;;) {
      // Reading sequentially is intentional: each iteration compares one chunk
      // and stops early on a mismatch or EOF.
      const [readA, readB] = await Promise.all([ // eslint-disable-line no-await-in-loop -- a mismatch or EOF in this chunk ends the comparison
        fhA.read(bufA, 0, FILE_COMPARE_CHUNK_SIZE, position),
        fhB.read(bufB, 0, FILE_COMPARE_CHUNK_SIZE, position),
      ])
      if (readA.bytesRead !== readB.bytesRead) return false
      if (readA.bytesRead === 0) return true
      if (!bufA.subarray(0, readA.bytesRead).equals(bufB.subarray(0, readB.bytesRead))) {
        return false
      }
      position += readA.bytesRead
    }
  } catch {
    // A transient read error must not abort bin linking: treat the files as
    // different so the caller falls back to the warn + remove + relink path.
    return false
  } finally {
    await fhA.close().catch(() => {})
    await fhB.close().catch(() => {})
  }
}

export async function isPathPresent (file: string): Promise<boolean> {
  try {
    await fs.lstat(file)
    return true
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') return false
    throw err
  }
}

export async function isMissing (file: string): Promise<boolean> {
  try {
    await fs.stat(file)
    return false
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT' || err.code === 'ENOTDIR') return true
    throw err
  }
}

export async function canSymlinkExecutable (file: string): Promise<boolean> {
  try {
    const realFile = await fs.realpath(file)
    if (path.dirname(realFile).split(path.sep).includes('node_modules')) return true
    return await isRunnableExecutable(realFile, await fs.stat(realFile))
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT' || err.code === 'ENOTDIR') return true
    throw err
  }
}

export async function ensureExecutableIfNeeded (file: string, opts?: { allowMissing?: boolean }): Promise<void> {
  const stat = await fs.stat(file).catch((err: any) => { // eslint-disable-line
    if (opts?.allowMissing && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return undefined
    throw err
  })
  if (stat == null) return
  if ((stat.mode & 0o111) !== 0o111 || await hasWindowsShebang(file)) {
    await ensureExecutable(file)
  }
}

// Only installed package files may be repaired. Resolve symlinks before checking
// because workspace and link: dependencies also appear under node_modules.
async function ensureExecutable (file: string): Promise<void> {
  const realFile = await fs.realpath(file)
  if (!path.dirname(realFile).split(path.sep).includes('node_modules')) return
  const stat = await fs.stat(realFile)
  if (await isRunnableExecutable(realFile, stat)) return
  try {
    // Add only the execute bits the target lacks, so a bin imported under a
    // strict umask keeps the read and write bits that umask gave it (pnpm/pnpm#3807).
    await fixBin(realFile, (stat.mode & 0o777) | 0o111)
  } catch (err: any) { // eslint-disable-line
    if (isPermissionError(err) && await isAlreadyRunnableExecutable(realFile)) return
    throw err
  }
}

function isPermissionError (err: NodeJS.ErrnoException): boolean {
  return err.code === 'EPERM' || err.code === 'EACCES' || err.code === 'EROFS'
}

async function isAlreadyRunnableExecutable (file: string): Promise<boolean> {
  const stat = await fs.stat(file).catch(() => undefined)
  return stat != null && isRunnableExecutable(file, stat)
}

async function isRunnableExecutable (file: string, stat: Stats): Promise<boolean> {
  return (stat.mode & 0o111) === 0o111 && !(await hasWindowsShebang(file))
}

// Detects a `#!`-shebang line terminated by CRLF, which fails to execute on
// POSIX. Mirrors bin-links' own fix-bin detection so a chmod failure on a
// read-only store is only swallowed when the bin is genuinely already correct.
async function hasWindowsShebang (file: string): Promise<boolean> {
  const fh = await fs.open(file, 'r').catch(() => undefined)
  if (fh == null) return false
  try {
    const buf = Buffer.alloc(2048)
    await fh.read(buf, 0, 2048, 0)
    return buf[0] === 0x23 /* # */ && buf[1] === 0x21 /* ! */ && /^#![^\n]+\r\n/.test(buf.toString())
  } catch {
    return false
  } finally {
    await fh.close().catch(() => {})
  }
}
