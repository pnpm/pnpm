import path = require('path')
import {Package} from '../types'
import mem = require('mem')
import loadJsonFile = require('load-json-file')

const cachedReadPkg = mem(loadJsonFile)

/**
 * Works identically to require('/path/to/file.json'), but safer.
 */
export default function requireJson (pkgJsonPath: string): Package {
  pkgJsonPath = path.resolve(pkgJsonPath)
  return cachedReadPkg(pkgJsonPath)
}

export const ignoreCache: Function = loadJsonFile
