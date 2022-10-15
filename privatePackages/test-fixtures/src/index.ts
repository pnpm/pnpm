import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'
import fsx from 'fs-extra'

export function fixtures (searchFromDir: string) {
  return {
    copy: copyFixture.bind(null, searchFromDir),
    find: findFixture.bind(null, searchFromDir),
    prepare: prepareFixture.bind(null, searchFromDir),
  }
}

function prepareFixture (searchFromDir: string, name: string): string {
  const dir = tempDir()
  copyFixture(searchFromDir, name, dir)
  return dir
}

function copyFixture (searchFromDir: string, name: string, dest: string): void {
  const fixturePath = findFixture(searchFromDir, name)
  if (!fixturePath) throw new Error(`${name} not found`)
  fsx.copySync(fixturePath, dest)
}

function findFixture (dir: string, name: string): string {
  const { root } = path.parse(dir)
  while (true) {
    const checkDir = path.join(dir, 'fixtures', name)
    if (fs.existsSync(checkDir)) return checkDir
    if (dir === root) throw new Error(`Local package "${name}" not found`)
    dir = path.dirname(dir)
  }
}
