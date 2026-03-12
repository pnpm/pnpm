import fs from 'fs'
import path from 'path'
import { jest } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

// renameOverwrite is mocked so we can verify it's called (or not called)
// and control its behavior in tests.
jest.mock('rename-overwrite', () => {
  const fn = Object.assign(jest.fn(), { sync: jest.fn() })
  return fn
})

// eslint-disable-next-line @typescript-eslint/no-var-requires, @typescript-eslint/no-require-imports
const renameOverwrite = require('rename-overwrite') as { sync: jest.Mock }
// eslint-disable-next-line @typescript-eslint/no-var-requires, @typescript-eslint/no-require-imports
const { importIndexedDir } = require('../src/importIndexedDir.js')

beforeEach(() => {
  renameOverwrite.sync.mockReset()
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

  const filenames: Record<string, string> = { 'package.json': srcFile }

  // Should not throw — target exists, path is content-addressed so content is correct
  importIndexedDir(fs.copyFileSync, newDir, filenames, { safeToSkip: true })

  expect(fs.existsSync(path.join(newDir, 'package.json'))).toBe(true)
  // When safeToSkip is true and the target already exists with matching content,
  // renameOverwrite.sync should not be called on any platform.
  expect(renameOverwrite.sync).not.toHaveBeenCalled()
})

test('safeToSkip creates dir when target does not exist', () => {
  const tmp = tempDir()
  const srcFile = path.join(tmp, 'src', 'index.js')
  const newDir = path.join(tmp, 'dest')

  // Create source file
  fs.mkdirSync(path.join(tmp, 'src'), { recursive: true })
  fs.writeFileSync(srcFile, 'content')

  const filenames: Record<string, string> = { 'index.js': srcFile }

  // Target doesn't exist — should create it
  importIndexedDir(fs.copyFileSync, newDir, filenames, { safeToSkip: true })

  expect(fs.existsSync(path.join(newDir, 'index.js'))).toBe(true)
})
