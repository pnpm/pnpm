import path from 'node:path'

import fs from '@pnpm/fs.graceful-fs'

const dirs = new Set()

export function writeFile (
  fileDest: string,
  buffer: Buffer,
  mode?: number
): void {
  makeDirForFile(fileDest)
  fs.writeFileSync(fileDest, buffer, { mode })
}

/**
 * Atomically creates a file only if it doesn't already exist (O_CREAT|O_EXCL).
 * Throws EEXIST if the file was created by another process concurrently.
 */
export function writeFileExclusive (
  fileDest: string,
  buffer: Buffer,
  mode?: number
): void {
  makeDirForFile(fileDest)
  fs.writeFileSync(fileDest, buffer, { mode, flag: 'wx' })
}

function makeDirForFile (fileDest: string): void {
  const dir = path.dirname(fileDest)
  if (!dirs.has(dir)) {
    fs.mkdirSync(dir, { recursive: true })
    dirs.add(dir)
  }
}
