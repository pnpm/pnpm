import fs from 'fs'
import path from 'path'
import { jest } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

// Mock renameOverwriteSync so we can verify it's called (or not called)
// and control its behavior in tests.
const renameOverwriteSyncMock = jest.fn()
jest.unstable_mockModule('rename-overwrite', () => ({
  renameOverwrite: jest.fn(),
  renameOverwriteSync: renameOverwriteSyncMock,
}))

const { importIndexedDir } = await import('../src/importIndexedDir.js')

beforeEach(() => {
  renameOverwriteSyncMock.mockReset()
})

test('safeToSkip skips when target has matching content (hardlink)', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'package.json')
  const newDir = path.join(tmp, 'dest')

  // Create source file in CAS
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, '{"name":"pkg","version":"1.0.0"}')

  // Pre-create target with hardlink to same source (concurrent import)
  fs.mkdirSync(newDir, { recursive: true })
  fs.linkSync(srcFile, path.join(newDir, 'package.json'))

  const filenames = new Map([['package.json', srcFile]])

  // Should skip — target has matching content
  importIndexedDir(fs.copyFileSync, newDir, filenames, { safeToSkip: true })

  expect(fs.existsSync(path.join(newDir, 'package.json'))).toBe(true)
  expect(renameOverwriteSyncMock).not.toHaveBeenCalled()
})

test('safeToSkip skips when target has matching content (copy)', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'index.js')
  const newDir = path.join(tmp, 'dest')

  // Create source file in CAS
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, 'module.exports = true')

  // Pre-create target with a copy of the same content (not a hardlink)
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'index.js'), 'module.exports = true')

  const filenames = new Map([['index.js', srcFile]])

  importIndexedDir(fs.copyFileSync, newDir, filenames, { safeToSkip: true })

  expect(fs.existsSync(path.join(newDir, 'index.js'))).toBe(true)
  expect(renameOverwriteSyncMock).not.toHaveBeenCalled()
})

test('safeToSkip falls back to renameOverwriteSync when files are missing', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'index.js')
  const newDir = path.join(tmp, 'dest')

  // Create source file
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, 'content')

  // Target exists but does NOT have the expected file (incomplete)
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'other-file.txt'), 'other')

  const filenames = new Map([['index.js', srcFile]])

  importIndexedDir(fs.copyFileSync, newDir, filenames, { safeToSkip: true })

  expect(renameOverwriteSyncMock).toHaveBeenCalled()
})

test('safeToSkip falls back to renameOverwriteSync when content differs', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'index.js')
  const newDir = path.join(tmp, 'dest')

  // Create source file
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, 'new content')

  // Target exists with different content
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'index.js'), 'old content')

  const filenames = new Map([['index.js', srcFile]])

  importIndexedDir(fs.copyFileSync, newDir, filenames, { safeToSkip: true })

  expect(renameOverwriteSyncMock).toHaveBeenCalled()
})
