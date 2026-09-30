import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'

export function getHashLink (globalDir: string, hash: string): string {
  return path.join(globalDir, hash)
}

export function resolveInstallDir (globalDir: string, hash: string): string | null {
  const linkPath = getHashLink(globalDir, hash)
  try {
    const stats = fs.lstatSync(linkPath)
    if (!stats.isSymbolicLink()) return null
    return fs.realpathSync(linkPath)
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

export function createInstallDir (globalDir: string): string {
  // Ensure the parent exists, then create the per-group dir *exclusively*
  // (no `recursive`, which would silently reuse an existing entry or follow
  // a pre-existing symlink). The name adds random bytes on top of pid+time
  // so it isn't predictable and can't collide within the same millisecond.
  fs.mkdirSync(globalDir, { recursive: true })
  for (let attempt = 0; attempt < 10; attempt++) {
    const name = `${process.pid.toString(16)}-${Date.now().toString(16)}-${crypto.randomBytes(8).toString('hex')}`
    const dir = path.join(globalDir, name)
    try {
      fs.mkdirSync(dir)
      return dir
    } catch (err) {
      if (isError(err) && 'code' in err && err.code === 'EEXIST') continue
      throw err
    }
  }
  throw new Error('Could not create a unique global install directory')
}
