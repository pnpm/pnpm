import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'

export interface SymlinkDirs {
  writtenDir: string
  finalDir: string
  imported: Set<string>
}

export function copyInternalSymlink (src: string, dest: string, links: SymlinkDirs): boolean {
  let target = fs.readlinkSync(src)
  if (path.isAbsolute(target)) {
    target = path.relative(realpathOrSelf(path.dirname(src)), realpathOrSelf(target))
  }
  const resolved = path.resolve(path.dirname(dest), target)
  if (escapesDir(links.writtenDir, resolved)) return false
  if (!links.imported.has(path.relative(links.writtenDir, resolved).split(path.sep).join('/'))) {
    return isDirectory(src)
  }
  if (process.platform !== 'win32') {
    fs.symlinkSync(target, dest)
    return true
  }
  return copyInternalSymlinkWindows({ dest, links, resolved, src, target })
}

interface CopySymlinkWindowsOptions {
  dest: string
  links: SymlinkDirs
  resolved: string
  src: string
  target: string
}

function copyInternalSymlinkWindows (opts: CopySymlinkWindowsOptions): boolean {
  const { dest, links, resolved, src, target } = opts
  const isDir = isDirectory(src)
  try {
    fs.symlinkSync(target, dest, isDir ? 'dir' : 'file')
    return true
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err) || err.code !== 'EPERM') throw err
    if (!isDir) return false
    fs.symlinkSync(path.join(links.finalDir, path.relative(links.writtenDir, resolved)), dest, 'junction')
    return true
  }
}

function realpathOrSelf (file: string): string {
  try {
    return fs.realpathSync(file)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return file
    throw err
  }
}

function isDirectory (file: string): boolean {
  try {
    return fs.statSync(file).isDirectory()
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}

function escapesDir (rootDir: string, targetPath: string): boolean {
  const rel = path.relative(rootDir, targetPath)
  return rel === '..' || rel.startsWith(`..${path.sep}`) || path.isAbsolute(rel)
}
