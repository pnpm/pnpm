import fs from 'fs'
import path from 'path'
import { tempDir as createTempDir } from '@pnpm/prepare'
import { hardLinkDir } from '@pnpm/fs.hard-link-dir'

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
