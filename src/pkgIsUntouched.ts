import fs = require('mz/fs')
import loadJsonFile = require('load-json-file')
import dint = require('dint')
import path = require('path')

export default async function untouched (pkgDir: string): Promise<false | {}> {
  let dirIntegrity: {} | null = null
  try {
    dirIntegrity = await loadJsonFile(path.join(path.dirname(pkgDir), 'integrity.json'))
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
    return false // for backward compatibility
  }
  return dint.check(pkgDir, dirIntegrity)
    .then((ok: boolean) => ok && dirIntegrity)
}
