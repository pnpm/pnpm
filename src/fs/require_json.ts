import path = require('path')
import fs = require('fs')

const cache = {}

/*
 * Works identically to require('/path/to/file.json'), but safer.
 */

export default function requireJson (pkgJsonPath, opts?) {
  opts = opts || {}
  pkgJsonPath = path.resolve(pkgJsonPath)
  if (!opts.ignoreCache && cache[pkgJsonPath]) return cache[pkgJsonPath]
  cache[pkgJsonPath] = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf-8'))
  return cache[pkgJsonPath]
}
