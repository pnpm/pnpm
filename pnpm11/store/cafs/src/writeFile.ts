import nodeFs from 'node:fs'
import path from 'node:path'

import fs from '@pnpm/fs.graceful-fs'

const dirs = new Set<string>()

export function writeFile (
  fileDest: string,
  buffer: Buffer,
  mode?: number
): void {
  makeDirForFile(fileDest)
  writeCreatedFile(fileDest, buffer, mode, false)
}

/**
 * Creates a file only if it doesn't already exist, using O_CREAT|O_EXCL.
 * Throws EEXIST if the file was created by another process concurrently.
 * Note: the write itself is not atomic — a crash mid-write can leave a partial file.
 */
export function writeFileExclusive (
  fileDest: string,
  buffer: Buffer,
  mode?: number
): void {
  makeDirForFile(fileDest)
  writeCreatedFile(fileDest, buffer, mode, true)
}

function writeCreatedFile (fileDest: string, buffer: Buffer, mode: number | undefined, exclusive: boolean): void {
  if (process.platform === 'win32') {
    fs.writeFileSync(fileDest, buffer, exclusive ? { mode, flag: 'wx' } : { mode })
    return
  }
  const creation = unixCreationMode(path.dirname(fileDest), mode)
  const options: { mode?: number, flag?: string } = {}
  if (creation.openMode != null) options.mode = creation.openMode
  if (exclusive) options.flag = 'wx'
  fs.writeFileSync(fileDest, buffer, options)
  if (creation.grantMode != null) grantModeBits(fileDest, creation.grantMode)
}

function makeDirForFile (fileDest: string): void {
  const dir = path.dirname(fileDest)
  if (dirs.has(dir)) return
  if (process.platform !== 'win32' && !directoryExists(dir)) {
    const template = nearestExistingAncestor(dir)
    fs.mkdirSync(dir, { recursive: true })
    if (template != null) grantInheritedDirMode(dir, template)
  } else {
    fs.mkdirSync(dir, { recursive: true })
  }
  dirs.add(dir)
}

// Read and write bits come from the parent directory, so a group-writable
// store stays group-writable. Execute bits are copied only for an executable
// file. Owner read and write stay set so the creator can finish the write.
function inheritedFileMode (parentMode: number, executable: boolean): number {
  let mode = parentMode & 0o666
  if (executable) {
    if ((parentMode & 0o100) !== 0) mode |= 0o100
    if ((parentMode & 0o010) !== 0) mode |= 0o010
    if ((parentMode & 0o001) !== 0) mode |= 0o001
  }
  return mode | 0o600
}

function isPrivateMode (mode: number | undefined): boolean {
  return mode != null && (mode & 0o077) === 0
}

// The open mode is a ceiling: umask can only remove bits, and a default ACL
// can grant only bits that are present. `grantMode` is added back afterwards
// without clearing bits the create already applied. A private mode such as
// `0o600` is not widened.
function unixCreationMode (parent: string, mode: number | undefined): { openMode: number | undefined, grantMode: number | undefined } {
  if (isPrivateMode(mode)) return { openMode: mode, grantMode: undefined }
  const executable = mode != null && (mode & 0o111) !== 0
  try {
    const wanted = inheritedFileMode(nodeFs.statSync(parent).mode, executable)
    return { openMode: wanted, grantMode: wanted }
  } catch (err: unknown) {
    if (isUnchangeable(err) || isMissing(err)) return { openMode: mode, grantMode: undefined }
    throw err
  }
}

function grantModeBits (filePath: string, wanted: number): void {
  let fd: number | undefined
  try {
    fd = nodeFs.openSync(filePath, nodeFs.constants.O_RDONLY | (nodeFs.constants.O_NOFOLLOW ?? 0))
    const current = nodeFs.fstatSync(fd).mode & 0o777
    const merged = current | (wanted & 0o777)
    if (merged !== current) nodeFs.fchmodSync(fd, merged)
  } catch (err: unknown) {
    if (!isUnchangeable(err) && !isMissing(err)) throw err
  } finally {
    if (fd != null) nodeFs.closeSync(fd)
  }
}

function directoryExists (dir: string): boolean {
  try {
    return nodeFs.statSync(dir).isDirectory()
  } catch (err: unknown) {
    if (isMissing(err)) return false
    throw err
  }
}

function nearestExistingAncestor (dir: string): string | undefined {
  let current = dir
  for (;;) {
    try {
      if (nodeFs.statSync(current).isDirectory()) return current
    } catch (err: unknown) {
      if (!isMissing(err)) {
        if (isUnchangeable(err)) return undefined
        throw err
      }
    }
    const parent = path.dirname(current)
    if (parent === current) return undefined
    current = parent
  }
}

// New directories only. `template` is the closest ancestor that already
// existed; it is not chmod'd, and neither is the filesystem root.
function grantInheritedDirMode (dir: string, template: string): void {
  let extra: number
  try {
    extra = nodeFs.statSync(template).mode & (0o020 | 0o2000)
  } catch (err: unknown) {
    if (isUnchangeable(err) || isMissing(err)) return
    throw err
  }
  if (extra === 0) return
  let current = dir
  while (current !== template) {
    const parent = path.dirname(current)
    if (parent === current) break
    try {
      const mode = nodeFs.statSync(current).mode & 0o7777
      const merged = mode | extra
      if (merged !== mode) nodeFs.chmodSync(current, merged)
    } catch (err: unknown) {
      if (!isUnchangeable(err) && !isMissing(err)) throw err
    }
    current = parent
  }
}

function isUnchangeable (err: unknown): boolean {
  const code = errorCode(err)
  return code === 'EPERM' || code === 'EACCES' || code === 'EROFS'
}

function isMissing (err: unknown): boolean {
  const code = errorCode(err)
  return code === 'ENOENT' || code === 'ENOTDIR'
}

function errorCode (err: unknown): string | undefined {
  if (typeof err === 'object' && err != null && 'code' in err) {
    const code = err.code
    return typeof code === 'string' ? code : undefined
  }
  return undefined
}
