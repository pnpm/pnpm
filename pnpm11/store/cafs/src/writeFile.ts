import nodeFs from 'node:fs'
import path from 'node:path'

import fs from '@pnpm/fs.graceful-fs'
import {
  directoryExists,
  grantInheritedDirMode,
  grantModeBits,
  nearestExistingAncestor,
  readDirMode,
  unixCreationMode,
} from '@pnpm/store.file-mode'

// Directories this process has ensured, mapped to their mode on POSIX.
// Windows entries hold no mode, so new files there keep the requested mode.
const dirModes = new Map<string, number | undefined>()

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
  const creation = unixCreationMode(dirModes.get(path.dirname(fileDest)), mode)
  if (creation.grantMode == null) {
    fs.writeFileSync(fileDest, buffer, exclusive ? { mode: creation.openMode, flag: 'wx' } : { mode: creation.openMode })
    return
  }
  const fd = nodeFs.openSync(fileDest, exclusive ? 'wx' : 'w', creation.openMode)
  try {
    grantModeBits(fd, creation.grantMode)
    fs.writeFileSync(fd, buffer)
  } finally {
    nodeFs.closeSync(fd)
  }
}

function makeDirForFile (fileDest: string): void {
  const dir = path.dirname(fileDest)
  if (dirModes.has(dir)) return
  if (process.platform === 'win32') {
    fs.mkdirSync(dir, { recursive: true })
    dirModes.set(dir, undefined)
    return
  }
  if (!directoryExists(dir)) {
    const template = nearestExistingAncestor(dir)
    fs.mkdirSync(dir, { recursive: true })
    if (template != null) grantInheritedDirMode(dir, template)
  }
  dirModes.set(dir, readDirMode(dir))
}
