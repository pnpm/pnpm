import path = require('path')
import fs = require('mz/fs')

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
  for (const dir of await fs.readdir(parentDir, { withFileTypes: true })) {
    if (dir.isFile() || dir.name[0] === '.') continue

    if (!scope && dir.name[0] === '@') {
      pkgNames = [
        ...pkgNames,
        ...await _readModulesDir(modulesDir, dir.name),
      ]
      continue
    }

    const pkgName = scope ? `${scope}/${dir.name}` : dir.name
    pkgNames.push(pkgName)
  }
  return pkgNames
}
