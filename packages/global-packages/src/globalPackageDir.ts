import fs from 'fs'
import path from 'path'
import util from 'util'

export function getGlobalDir (pnpmHomeDir: string): string {
  return path.join(pnpmHomeDir, '.global')
}

export function getHashDir (globalDir: string, hash: string): string {
  return path.join(globalDir, hash)
}

export function resolveActiveInstall (hashDir: string): string | null {
  const pkgLink = path.join(hashDir, 'pkg')
  try {
    const stats = fs.lstatSync(pkgLink)
    if (!stats.isSymbolicLink()) return null
    return fs.realpathSync(pkgLink)
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

export function getPrepareDir (hashDir: string): string {
  const name = `${new Date().getTime().toString(16)}-${process.pid.toString(16)}`
  return path.join(hashDir, name)
}
