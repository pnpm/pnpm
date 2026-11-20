import fs from 'node:fs'
import path from 'node:path'

import { tempDir } from '@pnpm/prepare-temp-dir'

export interface FixturesHandle {
  copy: (name: string, dest: string) => void
  find: (name: string) => string
  prepare: (name: string) => string
}

export function fixtures (searchFromDir: string): FixturesHandle {
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

function copyAndRename (src: string, dest: string): void {
  const entries = fs.readdirSync(src)

  for (const entry of entries) {
    const srcPath = path.join(src, entry)
    const destPath = path.join(dest, entry[0] === '_' ? entry.substring(1) : entry)
    const stats = fs.lstatSync(srcPath)

    if (stats.isSymbolicLink()) {
      copySymlink(srcPath, destPath)
    } else if (stats.isDirectory()) {
      copyDirectory(srcPath, destPath)
    } else if (stats.isFile()) {
      fs.copyFileSync(srcPath, destPath)
    }
  }
}

function copySymlink (srcPath: string, destPath: string): void {
  let linkTarget = fs.readlinkSync(srcPath)
  if (path.isAbsolute(linkTarget)) {
    linkTarget = path.relative(path.dirname(srcPath), linkTarget)
  }
  fs.symlinkSync(linkTarget, destPath, process.platform === 'win32' ? 'junction' : undefined)
}

function copyDirectory (srcPath: string, destPath: string): void {
  if (!fs.existsSync(destPath)) {
    fs.mkdirSync(destPath)
  }
  copyAndRename(srcPath, destPath)
}

function findFixture (dir: string, name: string): string {
  const { root } = path.parse(dir)
  while (true) {
    let checkDir = path.join(dir, 'fixtures', name)
    if (fs.existsSync(checkDir)) return checkDir
    checkDir = path.join(dir, '__fixtures__', name)
    if (fs.existsSync(checkDir)) return checkDir
    checkDir = path.join(dir, 'node_modules/@pnpm/tgz-fixtures/tgz', name)
    if (fs.existsSync(checkDir)) return checkDir
    if (dir === root) throw new Error(`Local package "${name}" not found`)
    dir = path.dirname(dir)
  }
}
