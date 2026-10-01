import fs from 'node:fs'
import path from 'node:path'

import gfs from '@pnpm/fs.graceful-fs'

// The open mode is a ceiling: umask can only remove bits, and a default ACL
// can grant only bits that are present. `grantMode` is added back afterwards
// without clearing bits the create already applied. A private mode such as
// `0o600` is not widened. Any other mode is replaced by the parent's
// inherited mode, keeping only whether it is executable. Without a known
// `parentMode` the requested mode is used unchanged.
export function unixCreationMode (parentMode: number | undefined, mode: number | undefined): { openMode: number | undefined, grantMode: number | undefined } {
  if (parentMode == null || isPrivateMode(mode)) return { openMode: mode, grantMode: undefined }
  const executable = mode != null && (mode & 0o111) !== 0
  const wanted = inheritedFileMode(parentMode, executable)
  return { openMode: wanted, grantMode: wanted }
}

function isPrivateMode (mode: number | undefined): boolean {
  return mode != null && (mode & 0o077) === 0
}

// Read bits and group-write come from the parent directory, so a
// group-writable store stays group-writable. Other-write is never copied: a
// world-writable sticky store protects its entries, not their contents.
// Execute bits are copied only for an executable file. Owner read and write
// stay set so the creator can finish the write.
function inheritedFileMode (parentMode: number, executable: boolean): number {
  let mode = parentMode & 0o664
  if (executable) {
    if ((parentMode & 0o100) !== 0) mode |= 0o100
    if ((parentMode & 0o010) !== 0) mode |= 0o010
    if ((parentMode & 0o001) !== 0) mode |= 0o001
  }
  return mode | 0o600
}

// Mode of `dir`, or undefined when it cannot be read.
export function readDirMode (dir: string): number | undefined {
  try {
    return fs.statSync(dir).mode
  } catch (err: unknown) {
    if (isUnchangeable(err) || isMissing(err)) return undefined
    throw err
  }
}

// OR `wanted` onto the open file. Bits already present are kept, so a
// default ACL wider than the directory mode survives. A file this process
// cannot chmod is left as created.
export function grantModeBits (fd: number, wanted: number): void {
  try {
    const current = fs.fstatSync(fd).mode & 0o777
    const merged = current | (wanted & 0o777)
    if (merged !== current) fs.fchmodSync(fd, merged)
  } catch (err: unknown) {
    if (!isUnchangeable(err)) throw err
  }
}

// Recursive mkdir that gives each directory it creates the group permission
// and setgid bits of the nearest ancestor that already existed. Directories
// that were already present are not modified.
export function mkdirInheritingMode (dir: string): void {
  if (process.platform === 'win32' || directoryExists(dir)) {
    gfs.mkdirSync(dir, { recursive: true })
    return
  }
  const template = nearestExistingAncestor(dir)
  gfs.mkdirSync(dir, { recursive: true })
  if (template != null) grantInheritedDirMode(dir, template)
}

function directoryExists (dir: string): boolean {
  try {
    return fs.statSync(dir).isDirectory()
  } catch (err: unknown) {
    if (isMissing(err)) return false
    throw err
  }
}

function nearestExistingAncestor (dir: string): string | undefined {
  let current = dir
  for (;;) {
    const isDir = isAccessibleDirectory(current)
    if (isDir === undefined) return undefined
    if (isDir) return current
    const parent = path.dirname(current)
    if (parent === current) return undefined
    current = parent
  }
}

function isAccessibleDirectory (dir: string): boolean | undefined {
  try {
    return fs.statSync(dir).isDirectory()
  } catch (err: unknown) {
    if (isUnchangeable(err)) return undefined
    if (isMissing(err)) return false
    throw err
  }
}

// New directories only. `template` is the closest ancestor that already
// existed; it is not chmod'd, and neither is the filesystem root.
function grantInheritedDirMode (dir: string, template: string): void {
  const templateMode = readDirMode(template)
  if (templateMode == null) return
  const extra = inheritedDirBits(templateMode)
  if (extra === 0) return
  let current = dir
  while (current !== template) {
    const parent = path.dirname(current)
    if (parent === current) break
    addDirModeBits(current, extra)
    current = parent
  }
}

// Group read and search come along with group-write, so a restrictive umask
// cannot leave a new directory group-writable but not searchable.
function inheritedDirBits (templateMode: number): number {
  if ((templateMode & (0o020 | 0o2000)) === 0) return 0
  return templateMode & (0o070 | 0o2000)
}

// The chmod goes through a descriptor opened without following a symlink, so
// a directory entry swapped for a symlink after mkdir is refused rather than
// followed to a directory outside the store.
function addDirModeBits (dir: string, extra: number): void {
  let fd: number
  try {
    fd = fs.openSync(dir, fs.constants.O_RDONLY | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW)
  } catch (err: unknown) {
    if (errorCode(err) === 'EACCES') {
      addUnreadableDirModeBits(dir, extra)
      return
    }
    if (isUnchangeable(err) || errorCode(err) === 'ENOENT') return
    throw err
  }
  try {
    const mode = fs.fstatSync(fd).mode & 0o7777
    const merged = mode | extra
    if (merged !== mode) fs.fchmodSync(fd, merged)
  } catch (err: unknown) {
    if (!isUnchangeable(err)) throw err
  } finally {
    fs.closeSync(fd)
  }
}

// A umask that removes owner read (such as 0o477) leaves a new directory its
// owner cannot open for reading. The mode is changed without following a
// symlink: through lchmod on macOS, and on Linux through an O_PATH handle,
// which needs no read access, via its /proc/self/fd entry. Only a directory
// this process owns is changed. Elsewhere the grant is skipped.
function addUnreadableDirModeBits (dir: string, extra: number): void {
  try {
    if (process.platform === 'darwin') {
      addModeBitsWithoutFollowing(dir, extra)
    } else if (process.platform === 'linux') {
      addModeBitsThroughPathHandle(dir, extra)
    }
  } catch (err: unknown) {
    if (!isUnchangeable(err) && errorCode(err) !== 'ENOENT') throw err
  }
}

function addModeBitsWithoutFollowing (dir: string, extra: number): void {
  const stat = fs.lstatSync(dir)
  if (!stat.isDirectory() || stat.uid !== process.getuid?.()) return
  const merged = (stat.mode & 0o7777) | extra
  if (merged !== (stat.mode & 0o7777)) fs.lchmodSync(dir, merged)
}

// Node does not export O_PATH. This is its value in the generic Linux ABI
// that every architecture Node supports uses.
const LINUX_O_PATH = 0o10000000

function addModeBitsThroughPathHandle (dir: string, extra: number): void {
  const fd = fs.openSync(dir, LINUX_O_PATH | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW)
  try {
    const stat = fs.fstatSync(fd)
    if (!stat.isDirectory() || stat.uid !== process.getuid?.()) return
    const merged = (stat.mode & 0o7777) | extra
    if (merged !== (stat.mode & 0o7777)) fs.chmodSync(`/proc/self/fd/${fd}`, merged)
  } finally {
    fs.closeSync(fd)
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
