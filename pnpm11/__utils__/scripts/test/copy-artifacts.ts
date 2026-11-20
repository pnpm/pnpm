import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'

import { stripWindowsOnlyFiles } from '../src/copy-artifacts.js'

describe('stripWindowsOnlyFiles', () => {
  let dist: string
  beforeEach(() => {
    dist = fs.mkdtempSync(path.join(os.tmpdir(), 'copy-artifacts-dist-'))
    createDist(dist)
  })
  afterEach(() => fs.rmSync(dist, { recursive: true, force: true }))

  test.each(['linux-x64', 'linux-x64-musl', 'linux-arm64', 'darwin-arm64'])(
    'drops the Windows-only files from the %s archive',
    (target) => {
      stripWindowsOnlyFiles(dist, target)

      expect(exists(dist, 'node-gyp-bin/node-gyp.cmd')).toBe(false)
      expect(exists(dist, 'vendor')).toBe(false)
    }
  )

  test('keeps the POSIX node-gyp wrapper and the bundle in a non-Windows archive', () => {
    stripWindowsOnlyFiles(dist, 'darwin-arm64')

    expect(exists(dist, 'node-gyp-bin/node-gyp')).toBe(true)
    expect(exists(dist, 'pnpm.mjs')).toBe(true)
  })

  test.each(['win32-x64', 'win32-arm64'])('keeps the Windows-only files in the %s archive', (target) => {
    stripWindowsOnlyFiles(dist, target)

    expect(exists(dist, 'node-gyp-bin/node-gyp.cmd')).toBe(true)
    expect(exists(dist, 'vendor/fastlist-0.3.0-x64.exe')).toBe(true)
    expect(exists(dist, 'vendor/fastlist-0.3.0-x86.exe')).toBe(true)
  })
})

function createDist (dir: string): void {
  const files = [
    'pnpm.mjs',
    'node-gyp-bin/node-gyp',
    'node-gyp-bin/node-gyp.cmd',
    'vendor/fastlist-0.3.0-x64.exe',
    'vendor/fastlist-0.3.0-x86.exe',
  ]
  for (const file of files) {
    const full = path.join(dir, file)
    fs.mkdirSync(path.dirname(full), { recursive: true })
    fs.writeFileSync(full, '')
  }
}

function exists (dir: string, file: string): boolean {
  return fs.existsSync(path.join(dir, file))
}

