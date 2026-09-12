import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { expect, jest, test } from '@jest/globals'
import gfs from '@pnpm/fs.graceful-fs'
import { tempDir } from '@pnpm/prepare'

import { importIndexedDir } from '../src/importIndexedDir.js'

test('importIndexedDir() keepModulesDir merges node_modules', async () => {
  const tmp = tempDir()
  fs.mkdirSync(path.join(tmp, 'src/node_modules/a'), { recursive: true })
  fs.writeFileSync(path.join(tmp, 'src/node_modules/a/index.js'), 'module.exports = 1')

  fs.mkdirSync(path.join(tmp, 'dest/node_modules/b'), { recursive: true })
  fs.writeFileSync(path.join(tmp, 'dest/node_modules/b/index.js'), 'module.exports = 1')

  const newDir = path.join(tmp, 'dest')
  const filenames = new Map([
    ['node_modules/a/index.js', path.join(tmp, 'src/node_modules/a/index.js')],
  ])
  importIndexedDir({ importFile: fs.linkSync, importFileAtomic: fs.linkSync }, newDir, filenames, { keepModulesDir: true })

  expect(fs.readdirSync(path.join(newDir, 'node_modules')).sort()).toEqual(['a', 'b'])
})

test('importIndexedDir() safeToSkip replaces a damaged file a linking importer would adopt', async () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(src, 'index.js'), 'module.exports = 1')

  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'index.js'), 'half-written')
  fs.writeFileSync(path.join(newDir, 'build.node'), 'output of an interrupted build')

  const filenames = new Map([
    ['index.js', path.join(src, 'index.js')],
    ['package.json', path.join(src, 'package.json')],
  ])
  importIndexedDir(linkingImporter, newDir, filenames, { safeToSkip: true })

  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('module.exports = 1')
  // A file the package does not declare is not ours to remove from a directory
  // other projects read.
  expect(fs.existsSync(path.join(newDir, 'build.node'))).toBe(true)
})

test('importIndexedDir() safeToSkip repairs a directory damaged after it was completed', async () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(src, 'index.js'), 'module.exports = 1')

  // The completion marker says nothing about the files placed before it.
  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(newDir, 'index.js'), 'corrupted after the import')

  const filenames = new Map([
    ['index.js', path.join(src, 'index.js')],
    ['package.json', path.join(src, 'package.json')],
  ])
  importIndexedDir(linkingImporter, newDir, filenames, { safeToSkip: true })

  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('module.exports = 1')
})

test('importIndexedDir() safeToSkip detects damage after the first comparison buffer', async () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  const newDir = path.join(tmp, 'dest')
  const sharedPrefix = Buffer.alloc(70_000, 'a')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(src, 'index.js'), Buffer.concat([sharedPrefix, Buffer.from('source')]))
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(newDir, 'index.js'), Buffer.concat([sharedPrefix, Buffer.from('target')]))

  importIndexedDir(linkingImporter, newDir, new Map([
    ['index.js', path.join(src, 'index.js')],
    ['package.json', path.join(src, 'package.json')],
  ]), { safeToSkip: true })

  expect(fs.readFileSync(path.join(newDir, 'index.js'))).toEqual(
    Buffer.concat([sharedPrefix, Buffer.from('source')])
  )
})

test('importIndexedDir() safeToSkip replaces a symlink to matching content', async () => {
  if (process.platform === 'win32') return
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(src, 'index.js'), 'module.exports = 1')
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'package.json'), '{"name":"pkg"}')
  fs.symlinkSync(path.join(src, 'index.js'), path.join(newDir, 'index.js'), 'file')

  importIndexedDir(linkingImporter, newDir, new Map([
    ['index.js', path.join(src, 'index.js')],
    ['package.json', path.join(src, 'package.json')],
  ]), { safeToSkip: true })

  expect(fs.lstatSync(path.join(newDir, 'index.js')).isSymbolicLink()).toBe(false)
  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('module.exports = 1')
})

test('importIndexedDir() does not treat zero file IDs as the same file', () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(src, { recursive: true })
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(src, 'index.js'), 'source')
  fs.writeFileSync(path.join(newDir, 'index.js'), 'target')

  const originalLstatSync = fs.lstatSync
  const originalStatSync = gfs.statSync
  const lstatSync = jest.spyOn(fs, 'lstatSync').mockImplementation(((filePath: fs.PathLike) => Object.assign(
    originalLstatSync(filePath, { bigint: true }), {
      dev: 0n,
      ino: 0n,
    })) as typeof fs.lstatSync)
  const statSync = jest.spyOn(gfs, 'statSync').mockImplementation(((filePath: fs.PathLike) => Object.assign(
    originalStatSync(filePath, { bigint: true }), {
      dev: 0n,
      ino: 0n,
    })) as typeof gfs.statSync)
  try {
    importIndexedDir(linkingImporter, newDir, new Map([
      ['index.js', path.join(src, 'index.js')],
    ]), { safeToSkip: true })
  } finally {
    lstatSync.mockRestore()
    statSync.mockRestore()
  }

  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('source')
})

test('importIndexedDir() adopts a matching file placed by a concurrent repair', async () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(src, 'index.js'), 'module.exports = 1')

  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(newDir, 'index.js'), 'damaged')

  const renameSync = jest.spyOn(fs, 'renameSync').mockImplementationOnce((tmpFile, dest) => {
    fs.copyFileSync(tmpFile, dest)
    throw Object.assign(new Error('another importer won'), { code: 'EEXIST' })
  })
  try {
    importIndexedDir(linkingImporter, newDir, new Map([
      ['index.js', path.join(src, 'index.js')],
      ['package.json', path.join(src, 'package.json')],
    ]), { safeToSkip: true })
  } finally {
    renameSync.mockRestore()
  }

  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('module.exports = 1')
  expect(fs.readdirSync(newDir).sort()).toEqual(['index.js', 'package.json'])
})

test('importIndexedDir() keeps a mismatching destination when its repair rename fails', async () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(src, 'index.js'), 'module.exports = 1')

  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(newDir, 'index.js'), 'damaged')

  const renameSync = jest.spyOn(fs, 'renameSync').mockImplementationOnce(() => {
    throw Object.assign(new Error('rename failed'), { code: 'EEXIST' })
  })
  try {
    expect(() => importIndexedDir(linkingImporter, newDir, new Map([
      ['index.js', path.join(src, 'index.js')],
      ['package.json', path.join(src, 'package.json')],
    ]), { safeToSkip: true })).toThrow('rename failed')
  } finally {
    renameSync.mockRestore()
  }

  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('damaged')
  expect(fs.readdirSync(newDir).sort()).toEqual(['index.js', 'package.json'])
})

test('importIndexedDir() safeToSkip clears a file where the package needs a directory', async () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(src, 'index.js'), 'module.exports = 1')

  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'lib'), 'a file where a directory belongs')

  const filenames = new Map([
    ['lib/nested/index.js', path.join(src, 'index.js')],
    ['package.json', path.join(src, 'package.json')],
  ])
  importIndexedDir(linkingImporter, newDir, filenames, { safeToSkip: true })

  expect(fs.readFileSync(path.join(newDir, 'lib/nested/index.js'), 'utf8')).toBe('module.exports = 1')
})

test('importIndexedDir() safeToSkip clears a directory where the package needs a file', async () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(src, 'index.js'), 'module.exports = 1')

  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(path.join(newDir, 'index.js'), { recursive: true })
  fs.writeFileSync(path.join(newDir, 'index.js', 'stray'), 'leftover')

  const filenames = new Map([
    ['index.js', path.join(src, 'index.js')],
    ['package.json', path.join(src, 'package.json')],
  ])
  importIndexedDir(linkingImporter, newDir, filenames, { safeToSkip: true })

  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('module.exports = 1')
})

test('importIndexedDir() rebuilds a local-dir target even when safeToSkip is set', async () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')

  // An injected local package is copied at install time, so its target holds
  // whatever the previous install copied — including files since deleted from
  // the source, which only a rebuild clears.
  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(newDir, 'removed-from-the-source.js'), 'stale')

  const filenames = new Map([['package.json', path.join(src, 'package.json')]])
  importIndexedDir(linkingImporter, newDir, filenames, { safeToSkip: true, resolvedFrom: 'local-dir' })

  expect(fs.existsSync(path.join(newDir, 'removed-from-the-source.js'))).toBe(false)
  expect(fs.readFileSync(path.join(newDir, 'package.json'), 'utf8')).toBe('{"name":"pkg"}')
})

test('importIndexedDir() safeToSkip repairs a directory holding a nested node_modules in place', async () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src')
  fs.mkdirSync(src, { recursive: true })
  fs.writeFileSync(path.join(src, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(src, 'index.js'), 'module.exports = 1')

  // A package with bundled dependencies ships its own node_modules/, and the
  // interrupted-build call shape asks for it to be kept. A directory repaired
  // in place keeps it by never removing anything.
  const newDir = path.join(tmp, 'dest')
  fs.mkdirSync(path.join(newDir, 'node_modules/bundled'), { recursive: true })
  fs.writeFileSync(path.join(newDir, 'node_modules/bundled/index.js'), 'bundled dependency')

  const filenames = new Map([
    ['index.js', path.join(src, 'index.js')],
    ['package.json', path.join(src, 'package.json')],
  ])
  importIndexedDir(linkingImporter, newDir, filenames, { keepModulesDir: true, safeToSkip: true })

  expect(fs.readFileSync(path.join(newDir, 'node_modules/bundled/index.js'), 'utf8')).toBe('bundled dependency')
  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('module.exports = 1')
  expect(fs.readFileSync(path.join(newDir, 'package.json'), 'utf8')).toBe('{"name":"pkg"}')
})

// The hardlink and clone importers treat an existing target as already
// imported, which is why a repair cannot rely on them to replace a file.
function linkAdoptingExisting (src: string, dest: string): void {
  try {
    fs.linkSync(src, dest)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST')) throw err
  }
}

const linkingImporter = { importFile: linkAdoptingExisting, importFileAtomic: linkAdoptingExisting }
