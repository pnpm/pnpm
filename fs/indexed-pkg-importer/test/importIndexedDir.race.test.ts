import fs from 'fs'
import path from 'path'
import { jest } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

// renameOverwrite is mocked because its real implementation handles ENOTEMPTY
// internally (rimraf + retry). The race condition we're testing only surfaces
// when renameOverwrite exhausts its retries due to concurrent threads
// repeatedly recreating the target, which can't be reproduced deterministically.
const renameOverwriteSyncMock = jest.fn()
jest.unstable_mockModule('rename-overwrite', () => ({
  default: { sync: renameOverwriteSyncMock },
}))

const { importIndexedDir } = await import('../src/importIndexedDir.js')

beforeEach(() => {
  renameOverwriteSyncMock.mockReset()
})

test('importIndexedDir succeeds when rename races with another thread (ENOTEMPTY)', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'index.js')
  const newDir = path.join(tmp, 'dest')

  // Create source file
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, 'content')

  // Pre-create target with expected content (simulating another thread completed first)
  fs.mkdirSync(newDir, { recursive: true })
  fs.writeFileSync(path.join(newDir, 'index.js'), 'content')

  const filenames = new Map([['index.js', srcFile]])

  renameOverwriteSyncMock.mockImplementation(() => {
    throw Object.assign(new Error('ENOTEMPTY: directory not empty'), { code: 'ENOTEMPTY' })
  })

  // Should not throw — the target already has the expected content
  importIndexedDir(fs.copyFileSync, newDir, filenames, {})

  expect(fs.existsSync(path.join(newDir, 'index.js'))).toBe(true)
})

test('importIndexedDir throws ENOTEMPTY when target does not have expected content', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'index.js')
  const newDir = path.join(tmp, 'dest')

  // Create source file
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, 'content')

  // Target exists but does NOT have the expected file
  fs.mkdirSync(newDir, { recursive: true })

  const filenames = new Map([['index.js', srcFile]])

  renameOverwriteSyncMock.mockImplementation(() => {
    throw Object.assign(new Error('ENOTEMPTY: directory not empty'), { code: 'ENOTEMPTY' })
  })

  expect(() => {
    importIndexedDir(fs.copyFileSync, newDir, filenames, {})
  }).toThrow('ENOTEMPTY')
})
