import path from 'node:path'

import fs from '@pnpm/fs.graceful-fs'
import {
  directoryExists,
  grantInheritedDirMode,
  grantModeBits,
  nearestExistingAncestor,
  unixCreationMode,
} from '@pnpm/store.file-mode'

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
