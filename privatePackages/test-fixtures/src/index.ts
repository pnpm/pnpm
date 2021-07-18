import { promisify } from 'util'
import fs from 'fs'
import path from 'path'
import ncpCB from 'ncp'

const ncp = promisify(ncpCB)

export async function copyFixture (fixtureName: string, dest: string, searchFromDir?: string) {
  const fixturePath = pathToLocalPkg(fixtureName, searchFromDir)
  if (!fixturePath) throw new Error(`${fixtureName} not found`)
  return ncp(fixturePath, dest)
}

export function pathToLocalPkg (pkgName: string, _dir?: string) {
  let dir = _dir ?? __dirname
  const { root } = path.parse(dir)
  while (true) {
    const checkDir = path.join(dir, 'fixtures', pkgName)
    if (fs.existsSync(checkDir)) return checkDir
    if (dir === root) throw new Error(`Local package "${pkgName}" not found`)
    dir = path.dirname(dir)
  }
}
