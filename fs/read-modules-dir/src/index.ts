import path from 'path'
import util from 'util'
import gracefulFs from 'graceful-fs'

const readdir = util.promisify(gracefulFs.readdir)

export async function readModulesDir (modulesDir: string) {
  try {
    return await _readModulesDir(modulesDir)
  } catch (err: any) { // eslint-disable-line
    if (err['code'] === 'ENOENT') return null
    throw err
  }
}

async function _readModulesDir (
  modulesDir: string,
  scope?: string
) {
  const pkgNames: string[] = []
  const parentDir = scope ? path.join(modulesDir, scope) : modulesDir
  await Promise.all((await readdir(parentDir, { withFileTypes: true })).map(async (dir) => {
    if (dir.isFile() || dir.name[0] === '.') return

    if (!scope && dir.name[0] === '@') {
      pkgNames.push(...await _readModulesDir(modulesDir, dir.name))
      return
    }

    const pkgName = scope ? `${scope}/${dir.name as string}` : dir.name
    pkgNames.push(pkgName)
  }))
  return pkgNames
}
