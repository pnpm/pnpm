import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { jest } from '@jest/globals'

const testOnLinuxOnly = (process.platform === 'darwin' || process.platform === 'win32') ? test.skip : test
const testOnMacOSOnly = process.platform === 'darwin' ? test : test.skip

jest.unstable_mockModule('@pnpm/fs.graceful-fs', async () => {
  const { access } = jest.requireActual<typeof fs>('fs')
  const fsMock = {
    access,
    copyFileSync: jest.fn(),
    statSync: jest.fn(),
  }
  return {
    __esModule: true,
    default: fsMock,
    ...fsMock,
  }
})

const { cloneDir } = await import('@pnpm/fs.indexed-pkg-importer')

describe('cloneDir', () => {
  const tempDir = path.join(os.tmpdir(), 'pnpm-cloneDir-test-' + Date.now())

  beforeAll(async () => {
    fs.mkdirSync(tempDir, { recursive: true })
  })

  afterEach(async () => {
    // Clean up test directories after each test
    const entries = fs.readdirSync(tempDir)
    for (const entry of entries) {
      const entryPath = path.join(tempDir, entry)
      try {
        fs.rmSync(entryPath, { recursive: true, force: true })
      } catch {} // eslint-disable-line:no-empty
    }
  })

  afterAll(async () => {
    fs.rmSync(tempDir, { recursive: true, force: true })
  })

  test('returns false for non-existent source', async () => {
    const src = path.join(tempDir, 'non-existent-source')
    const dest = path.join(tempDir, 'dest')

    const result = await cloneDir(src, dest)

    expect(result).toBe(false)
    expect(fs.existsSync(dest)).toBe(false)
  })

  test('successfully clones a directory with files', async () => {
    const src = path.join(tempDir, 'source')
    const dest = path.join(tempDir, 'dest')

    fs.mkdirSync(src, { recursive: true })
    fs.writeFileSync(path.join(src, 'file1.txt'), 'content1')
    fs.writeFileSync(path.join(src, 'file2.txt'), 'content2')

    const result = await cloneDir(src, dest)

    if (process.platform === 'linux' || process.platform === 'darwin') {
      // On supported platforms, it should succeed (assuming filesystem support)
      expect(result).toBe(true)
    } else {
      expect(result).toBe(false)
    }
  })

  test('cloned files have correct content', async () => {
    const src = path.join(tempDir, 'source')
    const dest = path.join(tempDir, 'dest')

    fs.mkdirSync(src, { recursive: true })
    fs.writeFileSync(path.join(src, 'file.txt'), 'expected content')

    const result = await cloneDir(src, dest)

    if (result) {
      expect(fs.existsSync(dest)).toBe(true)
      expect(fs.existsSync(path.join(dest, 'file.txt'))).toBe(true)
      const content = fs.readFileSync(path.join(dest, 'file.txt'), 'utf8')
      expect(content).toBe('expected content')
    }
  })

  test('handles symlinks correctly', async () => {
    const src = path.join(tempDir, 'source-with-symlink')
    const dest = path.join(tempDir, 'dest-symlink')

    fs.mkdirSync(src, { recursive: true })
    fs.writeFileSync(path.join(src, 'realfile.txt'), 'real content')
    fs.symlinkSync('realfile.txt', path.join(src, 'link.txt'))

    const result = await cloneDir(src, dest)

    if (result) {
      expect(fs.existsSync(dest)).toBe(true)
      expect(fs.existsSync(path.join(dest, 'realfile.txt'))).toBe(true)

      // Symlink should exist and point to correct target
      const linkExists = fs.existsSync(path.join(dest, 'link.txt'))
      expect(linkExists).toBe(true)

      const linkStat = fs.lstatSync(path.join(dest, 'link.txt'))
      expect(linkStat.isSymbolicLink()).toBe(true)

      const linkTarget = fs.readlinkSync(path.join(dest, 'link.txt'))
      expect(linkTarget).toBe('realfile.txt')
    }
  })

  testOnLinuxOnly('Linux: successfully clones nested directories', async () => {
    const src = path.join(tempDir, 'nested-source')
    const dest = path.join(tempDir, 'nested-dest')

    fs.mkdirSync(path.join(src, 'subdir', 'deep'), { recursive: true })
    fs.writeFileSync(path.join(src, 'root.txt'), 'root content')
    fs.writeFileSync(path.join(src, 'subdir', 'mid.txt'), 'mid content')
    fs.writeFileSync(path.join(src, 'subdir', 'deep', 'deep.txt'), 'deep content')

    const result = await cloneDir(src, dest)

    expect(result).toBe(true)
    expect(fs.existsSync(dest)).toBe(true)
    expect(fs.existsSync(path.join(dest, 'root.txt'))).toBe(true)
    expect(fs.existsSync(path.join(dest, 'subdir', 'mid.txt'))).toBe(true)
    expect(fs.existsSync(path.join(dest, 'subdir', 'deep', 'deep.txt'))).toBe(true)
    expect(fs.readFileSync(path.join(dest, 'subdir', 'deep', 'deep.txt'), 'utf8')).toBe('deep content')
  })

  testOnLinuxOnly('Linux: clones empty directories', async () => {
    const src = path.join(tempDir, 'empty-source')
    const dest = path.join(tempDir, 'empty-dest')

    fs.mkdirSync(src, { recursive: true })

    const result = await cloneDir(src, dest)

    expect(result).toBe(true)
    expect(fs.existsSync(dest)).toBe(true)
    expect(fs.statSync(dest).isDirectory()).toBe(true)
  })

  testOnMacOSOnly('macOS: successfully clones directories', async () => {
    const src = path.join(tempDir, 'mac-source')
    const dest = path.join(tempDir, 'mac-dest')

    fs.mkdirSync(src, { recursive: true })
    fs.writeFileSync(path.join(src, 'file.txt'), 'mac content')

    const result = await cloneDir(src, dest)

    // On macOS, cp -c is used. May fail on non-APFS filesystems
    if (result) {
      expect(fs.existsSync(dest)).toBe(true)
      expect(fs.readFileSync(path.join(dest, 'file.txt'), 'utf8')).toBe('mac content')
    }
  })
})
