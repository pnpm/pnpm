import { promisify } from 'util'
import fs from 'fs'
import path from 'path'
import ncpCB from 'ncp'
import { fileURLToPath } from 'url'

const DIRNAME = path.dirname(fileURLToPath(import.meta.url))

const ncp = promisify(ncpCB)

export async function copyFixture (fixtureName: string, dest: string) {
  const fixturePath = pathToLocalPkg(fixtureName)
  if (!fixturePath) throw new Error(`${fixtureName} not found`)
  return ncp(fixturePath, dest)
}

export function pathToLocalPkg (pkgName: string) {
  let dir = DIRNAME
  const { root } = path.parse(dir)
  while (true) {
    const checkDir = path.join(dir, 'fixtures', pkgName)
    if (fs.existsSync(checkDir)) return checkDir
    if (dir === root) throw new Error(`Local package "${pkgName}" not found`)
    dir = path.dirname(dir)
  }
}
