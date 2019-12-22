import ncpCB = require('ncp')
import path = require('path')
import { promisify } from 'util'

const ncp = promisify(ncpCB)

export function copyFixture (fixtureName: string, dest: string) {
  return ncp(pathToLocalPkg(fixtureName), dest)
}

export function pathToLocalPkg (pkgName: string) {
  return path.join(__dirname, '..', 'fixtures', pkgName)
}
