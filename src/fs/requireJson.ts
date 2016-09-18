import path = require('path')
import fs = require('fs')
import {Package} from '../api/initCmd'

type PackagesCache = {
  [path: string]: Package
}

export type RequireJsonOptions = {
  ignoreCache: boolean
}

const cache: PackagesCache = {}

/**
 * Works identically to require('/path/to/file.json'), but safer.
 */
export default function requireJson (pkgJsonPath: string, opts?: RequireJsonOptions): Package {
  opts = opts || {ignoreCache: false}
  pkgJsonPath = path.resolve(pkgJsonPath)
  if (!opts.ignoreCache && cache[pkgJsonPath]) return cache[pkgJsonPath]
  cache[pkgJsonPath] = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf-8'))
  return cache[pkgJsonPath]
}
