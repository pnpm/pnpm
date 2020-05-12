import fs = require('mz/fs')
import path = require('path')

export default async function readModulesDir (modulesDir: string) {
  try {
    return await _readModulesDir(modulesDir)
  } catch (err) {
    if (err['code'] === 'ENOENT') return null
    throw err
  }
}

async function _readModulesDir (
  modulesDir: string,
  scope?: string
) {
  let pkgNames: string[] = []
  const parentDir = scope ? path.join(modulesDir, scope) : modulesDir
  for (const dir of await fs.readdir(parentDir)) {
    if (dir[0] === '.') continue

    if (!scope && dir[0] === '@') {
      pkgNames = [
        ...pkgNames,
        ...await _readModulesDir(modulesDir, dir),
      ]
      continue
    }

    const pkgName = scope ? `${scope}/${dir}` : dir
    pkgNames.push(pkgName)
  }
  return pkgNames
}
