import fs = require('mz/fs')
import loadJsonFile = require('load-json-file')
import dint = require('dint')

export default async function untouched (pkgDir: string): Promise<Boolean> {
  let dirIntegrity: {} | null = null
  try {
    dirIntegrity = await loadJsonFile(`${pkgDir}_integrity.json`)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
    return false // for backward compatibility
  }
  return dint.check(pkgDir, dirIntegrity)
}
