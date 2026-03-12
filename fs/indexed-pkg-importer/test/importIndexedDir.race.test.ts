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

test('safeToSkip skips when target already exists (content-addressed)', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'package.json')
  const newDir = path.join(tmp, 'dest')

  // Create source file in CAS
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, '{"name":"pkg","version":"1.0.0"}')

  // Pre-create target (simulates another process that already placed it)
  fs.mkdirSync(newDir, { recursive: true })
  fs.linkSync(srcFile, path.join(newDir, 'package.json'))

  const filenames = new Map([['package.json', srcFile]])

  // Should skip — target exists, path is content-addressed so content is correct
  importIndexedDir(fs.copyFileSync, newDir, filenames, { safeToSkip: true })

  expect(fs.existsSync(path.join(newDir, 'package.json'))).toBe(true)
  expect(renameOverwriteSyncMock).not.toHaveBeenCalled()
})

test('safeToSkip never calls renameOverwriteSync (even with different content)', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'index.js')
  const newDir = path.join(tmp, 'dest')

  // Create source file
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, 'new content')

  // Target exists with different content — but in a content-addressed store,
  // the hash guarantees correctness, so we trust the existing dir and never
  // use the destructive renameOverwriteSync.
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'index.js'), 'old content')

  const filenames = new Map([['index.js', srcFile]])

  importIndexedDir(fs.copyFileSync, newDir, filenames, { safeToSkip: true })

  expect(renameOverwriteSyncMock).not.toHaveBeenCalled()
})

test('safeToSkip creates dir when target does not exist', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'index.js')
  const newDir = path.join(tmp, 'dest')

  // Create source file
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, 'content')

  const filenames = new Map([['index.js', srcFile]])

  // Target doesn't exist — should create it via rename
  importIndexedDir(fs.copyFileSync, newDir, filenames, { safeToSkip: true })

  expect(fs.existsSync(path.join(newDir, 'index.js'))).toBe(true)
  expect(renameOverwriteSyncMock).not.toHaveBeenCalled()
})
