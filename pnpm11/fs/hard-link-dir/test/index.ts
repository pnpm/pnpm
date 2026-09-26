import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import { tempDir as createTempDir } from '@pnpm/prepare'

test('hardLinkDirectory()', () => {
  const tempDir = createTempDir()
  const srcDir = path.join(tempDir, 'source')
  const dest1Dir = path.join(tempDir, 'dest1')
  const dest2Dir = path.join(tempDir, 'dest2')

  fs.mkdirSync(srcDir, { recursive: true })
  fs.mkdirSync(dest1Dir, { recursive: true })
  fs.mkdirSync(path.join(srcDir, 'node_modules'), { recursive: true })
  fs.mkdirSync(path.join(srcDir, 'subdir'), { recursive: true })

  fs.writeFileSync(path.join(srcDir, 'file.txt'), 'Hello World')
  fs.writeFileSync(path.join(srcDir, 'subdir/file.txt'), 'Hello World')
  fs.writeFileSync(path.join(srcDir, 'node_modules/file.txt'), 'Hello World')

  hardLinkDir(srcDir, [dest1Dir, dest2Dir])

  // It should link the files from the root
  expect(fs.readFileSync(path.join(dest1Dir, 'file.txt'), 'utf8')).toBe('Hello World')
  expect(fs.readFileSync(path.join(dest2Dir, 'file.txt'), 'utf8')).toBe('Hello World')

  // It should link files from a subdirectory
  expect(fs.readFileSync(path.join(dest1Dir, 'subdir/file.txt'), 'utf8')).toBe('Hello World')
  expect(fs.readFileSync(path.join(dest2Dir, 'subdir/file.txt'), 'utf8')).toBe('Hello World')

  // It should not link files from node_modules
  expect(fs.existsSync(path.join(dest1Dir, 'node_modules/file.txt'))).toBe(false)
  expect(fs.existsSync(path.join(dest2Dir, 'node_modules/file.txt'))).toBe(false)
})

test("don't fail on missing source and dest directories", () => {
  const tempDir = createTempDir()
  const missingDirSrc = path.join(tempDir, 'missing_source')
  const missingDirDest = path.join(tempDir, 'missing_dest')

  hardLinkDir(missingDirSrc, [missingDirDest])

  // It should create an empty dest dir if src does not exist
  expect(fs.existsSync(missingDirSrc)).toBe(false)
  expect(fs.existsSync(missingDirDest)).toBe(true)
})

// A hoisted destination owns a node_modules that the source does not have: the
// dependencies that could not be hoisted any higher. The copies of one build
// chunk run concurrently, so a sibling package may be staging its own copy in
// there while this one is written, and its rename needs to still find it.
// See https://github.com/pnpm/pnpm/issues/12880
test("the destination's own node_modules survives", () => {
  const tempDir = createTempDir()
  const srcDir = path.join(tempDir, 'source')
  const destDir = path.join(tempDir, 'dest')

  fs.mkdirSync(path.join(srcDir, 'subdir'), { recursive: true })
  fs.writeFileSync(path.join(srcDir, 'file.txt'), 'built')
  fs.writeFileSync(path.join(srcDir, 'subdir/file.txt'), 'built')

  const nestedDep = path.join(destDir, 'node_modules/dep')
  const stagedSiblingCopy = path.join(destDir, 'node_modules/_tmp_1_2')
  fs.mkdirSync(nestedDep, { recursive: true })
  fs.mkdirSync(stagedSiblingCopy, { recursive: true })
  fs.writeFileSync(path.join(nestedDep, 'index.js'), 'nested dependency')
  fs.mkdirSync(path.join(destDir, 'subdir'), { recursive: true })
  fs.writeFileSync(path.join(destDir, 'file.txt'), 'not built yet')
  fs.writeFileSync(path.join(destDir, 'subdir/file.txt'), 'not built yet')

  hardLinkDir(srcDir, [destDir])

  expect(fs.readFileSync(path.join(destDir, 'file.txt'), 'utf8')).toBe('built')
  expect(fs.readFileSync(path.join(destDir, 'subdir/file.txt'), 'utf8')).toBe('built')
  expect(fs.readFileSync(path.join(nestedDep, 'index.js'), 'utf8')).toBe('nested dependency')
  expect(fs.existsSync(stagedSiblingCopy)).toBe(true)
})

test('a file that the destination already has is replaced', () => {
  const tempDir = createTempDir()
  const srcDir = path.join(tempDir, 'source')
  const destDir = path.join(tempDir, 'dest')

  fs.mkdirSync(srcDir, { recursive: true })
  fs.mkdirSync(destDir, { recursive: true })
  fs.writeFileSync(path.join(srcDir, 'file.txt'), 'built')
  fs.writeFileSync(path.join(destDir, 'file.txt'), 'not built yet')

  hardLinkDir(srcDir, [destDir])

  expect(fs.readdirSync(destDir)).toStrictEqual(['file.txt'])
  expect(fs.readFileSync(path.join(destDir, 'file.txt'), 'utf8')).toBe('built')
})

// A build can turn one of the package's own entries into the other kind. The
// destination still holds the pre-build copy, so the entry it has to make way
// for is of the wrong kind.
test('an entry that changed between a file and a directory is replaced', () => {
  const tempDir = createTempDir()
  const srcDir = path.join(tempDir, 'source')
  const destDir = path.join(tempDir, 'dest')

  fs.mkdirSync(path.join(srcDir, 'generated'), { recursive: true })
  fs.writeFileSync(path.join(srcDir, 'generated/index.js'), 'built')
  fs.writeFileSync(path.join(srcDir, 'placeholder.js'), 'built')

  fs.mkdirSync(path.join(destDir, 'placeholder.js'), { recursive: true })
  fs.writeFileSync(path.join(destDir, 'placeholder.js/leftover.js'), 'not built yet')
  fs.writeFileSync(path.join(destDir, 'generated'), 'not built yet')

  hardLinkDir(srcDir, [destDir])

  expect(fs.readFileSync(path.join(destDir, 'generated/index.js'), 'utf8')).toBe('built')
  expect(fs.readFileSync(path.join(destDir, 'placeholder.js'), 'utf8')).toBe('built')
})

test('a symlinked directory in the destination does not redirect the writes', () => {
  const tempDir = createTempDir()
  const srcDir = path.join(tempDir, 'source')
  const destDir = path.join(tempDir, 'dest')
  const outside = path.join(tempDir, 'outside')

  fs.mkdirSync(path.join(srcDir, 'lib'), { recursive: true })
  fs.writeFileSync(path.join(srcDir, 'lib/index.js'), 'built')

  fs.mkdirSync(outside, { recursive: true })
  fs.mkdirSync(destDir, { recursive: true })
  fs.symlinkSync(outside, path.join(destDir, 'lib'), 'junction')

  hardLinkDir(srcDir, [destDir])

  expect(fs.readFileSync(path.join(destDir, 'lib/index.js'), 'utf8')).toBe('built')
  expect(fs.lstatSync(path.join(destDir, 'lib')).isSymbolicLink()).toBe(false)
  expect(fs.readdirSync(outside)).toStrictEqual([])
})
