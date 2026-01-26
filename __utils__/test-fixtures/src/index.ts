import fs from 'fs'
import path from 'path'
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
    // Use lstatSync to avoid following symlinks - this prevents ENOENT errors
    // when symlink targets haven't been copied yet (e.g., when copying directories
    // in alphabetical order where a symlink points to a directory that comes later)
    const stats = fs.lstatSync(srcPath)

    if (stats.isSymbolicLink()) {
      // Recreate symlinks to preserve the pnpm node_modules structure
      let linkTarget = fs.readlinkSync(srcPath)

      // On Windows, junctions store absolute paths internally.
      // We need to convert them to paths relative to the new destination.
      if (path.isAbsolute(linkTarget)) {
        // Compute relative path from the original symlink to its target
        const relativeTarget = path.relative(path.dirname(srcPath), linkTarget)
        linkTarget = relativeTarget
      }

      // On Windows, pnpm uses junctions for directories to avoid permission issues.
      // We use 'junction' type on Windows for all symlinks since pnpm fixtures
      // typically contain directory symlinks in node_modules.
      fs.symlinkSync(linkTarget, destPath, process.platform === 'win32' ? 'junction' : undefined)
    } else if (stats.isDirectory()) {
      // If the entry is a directory, recursively copy its contents
      if (!fs.existsSync(destPath)) {
        fs.mkdirSync(destPath)
      }
      copyAndRename(srcPath, destPath)
    } else if (stats.isFile()) {
      fs.copyFileSync(srcPath, destPath)
    }
  }
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
