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

test('safeToSkip skips importing when the completion marker already exists', () => {
  const tmp = tempDir()
  const srcDir = path.join(tmp, 'src')
  const newDir = path.join(tmp, 'dest')
  const srcPackageJson = path.join(srcDir, 'package.json')
  const srcIndexJs = path.join(srcDir, 'index.js')

  fs.mkdirSync(srcDir, { recursive: true })
  fs.writeFileSync(srcPackageJson, '{"name":"pkg","version":"1.0.0"}')
  fs.writeFileSync(srcIndexJs, 'module.exports = 1')

  fs.mkdirSync(newDir, { recursive: true })
  fs.copyFileSync(srcPackageJson, path.join(newDir, 'package.json'))
  fs.copyFileSync(srcIndexJs, path.join(newDir, 'index.js'))

  const importFileMock = jest.fn(() => {
    throw new Error('importer should not run when the completion marker already exists')
  })
  const filenames = new Map([
    ['index.js', srcIndexJs],
    ['package.json', srcPackageJson],
  ])

  importIndexedDir({ importFile: importFileMock, importFileAtomic: importFileMock }, newDir, filenames, { safeToSkip: true })

  expect(importFileMock).not.toHaveBeenCalled()
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

  // Target doesn't exist — should create it
  importIndexedDir({ importFile: fs.copyFileSync, importFileAtomic: fs.copyFileSync }, newDir, filenames, { safeToSkip: true })

  expect(fs.existsSync(path.join(newDir, 'index.js'))).toBe(true)
})
