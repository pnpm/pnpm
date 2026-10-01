import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { lstatWithRetry, unlinkWithRetry } from '@pnpm/fs.graceful-fs'
import { rimrafSync } from '@zkochan/rimraf'

export function clearDirentBlockingDir (newDir: string, relativeDir: string): void {
  let dir = newDir
  for (const segment of relativeDir.split(/[\\/]/)) {
    dir = path.join(dir, segment)
    if (!clearDirentSegment(dir)) return
  }
}

function clearDirentSegment (dir: string): boolean {
  let stats
  try {
    stats = lstatWithRetry(dir)
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
  if (stats.isDirectory()) return true
  try {
    unlinkWithRetry(dir)
  } catch (err) {
    if (!dirFitsAt(dir)) throw err
  }
  return false
}

function dirFitsAt (dir: string): boolean {
  try {
    return lstatWithRetry(dir).isDirectory()
  } catch (err) {
    return isError(err) && 'code' in err && err.code === 'ENOENT'
  }
}

export function clearDirBlockingFile (dest: string): void {
  let stats
  try {
    stats = lstatWithRetry(dest)
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
  if (stats.isDirectory()) {
    rimrafSync(dest)
  }
}

export function makeFileMapDirs (
  newDir: string,
  filenames: Map<string, string>,
  opts?: { clearBlockers: boolean }
): void {
  const allDirs = new Set<string>()
  for (const relativePath of filenames.keys()) {
    const dir = path.dirname(relativePath)
    if (dir === '.') continue
    allDirs.add(dir)
  }
  for (const dir of Array.from(allDirs).sort((d1, d2) => d1.length - d2.length)) {
    if (opts?.clearBlockers) {
      clearDirentBlockingDir(newDir, dir)
    }
    fs.mkdirSync(path.join(newDir, dir), { recursive: true })
  }
}
