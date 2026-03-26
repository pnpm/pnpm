import fs from 'node:fs'
import path from 'node:path'

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

  // Should not throw — target exists, path is content-addressed so content is correct
  importIndexedDir({ importFile: fs.copyFileSync, importFileAtomic: fs.copyFileSync }, newDir, filenames, { safeToSkip: true })

  expect(fs.existsSync(path.join(newDir, 'package.json'))).toBe(true)
  // When safeToSkip is true and the target already exists with matching content,
  // renameOverwriteSync should not be called on any platform.
  expect(renameOverwriteSyncMock).not.toHaveBeenCalled()
})

test('non-safeToSkip falls through to staging when target already exists', () => {
  const tmp = tempDir()
  const srcDir = path.join(tmp, 'src')
  fs.mkdirSync(srcDir, { recursive: true })

  const srcPkgJson = path.join(srcDir, 'package.json')
  const srcIndex = path.join(srcDir, 'index.js')
  fs.writeFileSync(srcPkgJson, '{"name":"pkg","version":"2.0.0"}')
  fs.writeFileSync(srcIndex, 'v2')

  const newDir = path.join(tmp, 'dest')

  // Pre-create target with a stale file from a previous version
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'package.json'), '{"name":"pkg","version":"1.0.0"}')
  fs.writeFileSync(path.join(newDir, 'index.js'), 'v1')
  fs.writeFileSync(path.join(newDir, 'stale.js'), 'should be removed')

  const filenames = new Map([
    ['package.json', srcPkgJson],
    ['index.js', srcIndex],
  ])

  // renameOverwriteSync replaces the directory atomically in real code.
  // Mock it to simulate the staging path completing.
  renameOverwriteSyncMock.mockImplementation((stage: string, dest: string) => {
    fs.rmSync(dest, { recursive: true })
    fs.renameSync(stage, dest)
  })

  importIndexedDir({ importFile: fs.copyFileSync, importFileAtomic: fs.copyFileSync }, newDir, filenames, { safeToSkip: false })

  // Staging path should have been used (renameOverwriteSync called)
  expect(renameOverwriteSyncMock).toHaveBeenCalled()
  // New files should be present
  expect(fs.readFileSync(path.join(newDir, 'index.js'), 'utf8')).toBe('v2')
  // Stale file should be gone (full directory replacement)
  expect(fs.existsSync(path.join(newDir, 'stale.js'))).toBe(false)
})

test('fast path does not empty directory created by concurrent importer', () => {
  const tmp = tempDir()
  const srcDir = path.join(tmp, 'src')
  fs.mkdirSync(srcDir, { recursive: true })

  const srcPkgJson = path.join(srcDir, 'package.json')
  fs.writeFileSync(srcPkgJson, '{"name":"pkg"}')

  const newDir = path.join(tmp, 'dest')

  // Pre-create target (simulates a concurrent importer that created the dir)
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'package.json'), '{"name":"pkg"}')
  fs.writeFileSync(path.join(newDir, 'index.js'), 'concurrent write in progress')

  const filenames = new Map([['package.json', srcPkgJson]])

  renameOverwriteSyncMock.mockImplementation((stage: string, dest: string) => {
    fs.rmSync(dest, { recursive: true })
    fs.renameSync(stage, dest)
  })

  importIndexedDir({ importFile: fs.copyFileSync, importFileAtomic: fs.copyFileSync }, newDir, filenames, { safeToSkip: false })

  // The concurrent importer's extra file should NOT be wiped by the fast path.
  // Instead, the staging path should have atomically replaced the directory.
  expect(renameOverwriteSyncMock).toHaveBeenCalled()
})

test('safeToSkip creates dir when target does not exist', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'index.js')
  const newDir = path.join(tmp, 'dest')

  // Create source file
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, 'content')

  const filenames = new Map([['index.js', srcFile]])

  // Target doesn't exist — should create it
  importIndexedDir({ importFile: fs.copyFileSync, importFileAtomic: fs.copyFileSync }, newDir, filenames, { safeToSkip: true })

  expect(fs.existsSync(path.join(newDir, 'index.js'))).toBe(true)
})
