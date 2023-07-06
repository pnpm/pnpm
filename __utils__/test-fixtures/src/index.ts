import fs from 'fs'
import path from 'path'
import { tempDir } from '@pnpm/prepare'

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
  const stats = fs.statSync(fixturePath)
  if (stats.isDirectory()) {
    fs.mkdirSync(dest, { recursive: true })
    copyAndRename(fixturePath, dest)
  } else {
    fs.mkdirSync(path.dirname(dest), { recursive: true })
    fs.copyFileSync(fixturePath, dest)
  }
}

function copyAndRename (src: string, dest: string) {
  const entries = fs.readdirSync(src)

  entries.forEach(entry => {
    const srcPath = path.join(src, entry)
    const destPath = path.join(dest, entry.startsWith('_') ? entry.substring(1) : entry)
    const stats = fs.statSync(srcPath)

    if (stats.isDirectory()) {
      // If the entry is a directory, recursively copy its contents
      if (!fs.existsSync(destPath)) {
        fs.mkdirSync(destPath)
      }
      copyAndRename(srcPath, destPath)
    } else if (stats.isFile()) {
      fs.copyFileSync(srcPath, destPath)
    }
  })
}

function findFixture (dir: string, name: string): string {
  const { root } = path.parse(dir)
  while (true) {
    let checkDir = path.join(dir, 'fixtures', name)
    if (fs.existsSync(checkDir)) return checkDir
    checkDir = path.join(dir, '__fixtures__', name)
    if (fs.existsSync(checkDir)) return checkDir
    if (dir === root) throw new Error(`Local package "${name}" not found`)
    dir = path.dirname(dir)
  }
}
