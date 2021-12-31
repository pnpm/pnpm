import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import fsx from 'fs-extra'

export default function (searchFromDir: string) {
  return {
    copy: copyFixtureSync.bind(null, searchFromDir),
    find: pathToLocalPkg.bind(null, searchFromDir),
    prepare: prepareFixture.bind(null, searchFromDir),
  }
}

function prepareFixture (searchFromDir: string, fixtureName: string): string {
  const dir = tempDir()
  copyFixtureSync(searchFromDir, fixtureName, dir)
  return dir
}

function copyFixtureSync (searchFromDir: string, fixtureName: string, dest: string) {
  const fixturePath = pathToLocalPkg(searchFromDir, fixtureName)
  if (!fixturePath) throw new Error(`${fixtureName} not found`)
  return fsx.copySync(fixturePath, dest)
}

function pathToLocalPkg (dir: string, pkgName: string) {
  const { root } = path.parse(dir)
  while (true) {
    const checkDir = path.join(dir, 'fixtures', pkgName)
    if (fs.existsSync(checkDir)) return checkDir
    if (dir === root) throw new Error(`Local package "${pkgName}" not found`)
    dir = path.dirname(dir)
  }
}
