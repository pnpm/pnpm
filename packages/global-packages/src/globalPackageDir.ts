import fs from 'fs'
import path from 'path'
import util from 'util'

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
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

export function createInstallDir (globalDir: string): string {
  const name = `${process.pid.toString(16)}-${Date.now().toString(16)}`
  const dir = path.join(globalDir, name)
  fs.mkdirSync(dir, { recursive: true })
  return dir
}
