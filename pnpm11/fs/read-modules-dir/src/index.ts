import path from 'node:path'
import util from 'node:util'

import gracefulFs from 'graceful-fs'

const readdir = util.promisify(gracefulFs.readdir)

export async function readModulesDir (modulesDir: string): Promise<string[] | null> {
  try {
    return await _readModulesDir(modulesDir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null
    throw err
  }
}

async function _readModulesDir (
  modulesDir: string,
  scope?: string
): Promise<string[]> {
  const pkgNames: string[] = []
  const parentDir = scope ? path.join(modulesDir, scope) : modulesDir
  await Promise.all((await readdir(parentDir, { withFileTypes: true })).map(async (dir) => {
    if (dir.isFile() || dir.name[0] === '.') return

    if (!scope && dir.name[0] === '@') {
      // Names below a symlinked scope container reach their target through the
      // symlink, wherever it points — a caller that deletes what it enumerates
      // follows it out of `modulesDir`. pnpm only ever symlinks the packages
      // inside a scope, never the scope itself, so skipping costs nothing.
      if (dir.isSymbolicLink()) return
      pkgNames.push(...await _readModulesDir(modulesDir, dir.name))
      return
    }

    const pkgName = scope ? `${scope}/${dir.name as string}` : dir.name
    pkgNames.push(pkgName)
  }))
  return pkgNames
}
