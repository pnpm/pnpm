import { promisify } from 'util'
import fs = require('fs')
import path = require('path')
import ncpCB = require('ncp')

const ncp = promisify(ncpCB)

export function copyFixture (fixtureName: string, dest: string) {
  const fixturePath = pathToLocalPkg(fixtureName)
  if (!fixturePath) throw new Error(`${fixtureName} not found`)
  return ncp(fixturePath, dest)
}

export function pathToLocalPkg (pkgName: string) {
  let dir = __dirname
  const { root } = path.parse(dir)
  while (true) {
    const checkDir = path.join(dir, 'fixtures', pkgName)
    if (fs.existsSync(checkDir)) return checkDir
    if (dir === root) throw new Error(`Local package "${pkgName}" not found`)
    dir = path.dirname(dir)
  }
}
