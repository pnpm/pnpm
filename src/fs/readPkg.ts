import path = require('path')
import {Package} from '../types'
import mem = require('mem')
import readPkg = require('read-pkg')

const cachedReadPkg = mem(readPkg)

/**
 * Works identically to require('/path/to/file.json'), but safer.
 */
export default function (pkgJsonPath: string): Promise<Package> {
  pkgJsonPath = path.resolve(pkgJsonPath)
  return cachedReadPkg(pkgJsonPath)
}

export const ignoreCache: Function = readPkg
