// cspell:ignore mkfifo
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'

import { afterEach, expect, jest, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import isWindows from 'is-windows'

import { DIR, DirPatcher, extendFilesMap, type ExtendFilesMapStats, type InodeMap } from '../src/DirPatcher.js'

// `mkfifo` has no Windows equivalent; the stats-driven test above it covers
// the same branch on every platform.
const testOnPosix = isWindows() ? test.skip : test

const originalStat = fs.promises.stat

function mockFsPromiseStat (): jest.Mock {
  const mockedMethod = jest.fn(fs.promises.stat)
  fs.promises.stat = mockedMethod as typeof fs.promises.stat
  return mockedMethod as jest.Mock
}

afterEach(() => {
  jest.restoreAllMocks()
  fs.promises.stat = originalStat
})

test('without provided stats', async () => {
  prepareEmpty()

  const filePaths = [
    'deep/a/b/c/d/e/f.txt',
    'foo/foo.txt',
    'foo/bar.txt',
    'foo_bar.txt',
  ]
  const filesMap = new Map<string, string>()
  for (const filePath of filePaths) {
    filesMap.set(filePath, path.resolve(filePath))
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
    fs.writeFileSync(filePath, '')
  }

  const statMethod = mockFsPromiseStat()

  expect(await extendFilesMap({ filesMap })).toStrictEqual({
    '.': DIR,
    deep: DIR,
    'deep/a': DIR,
    'deep/a/b': DIR,
    'deep/a/b/c': DIR,
    'deep/a/b/c/d': DIR,
    'deep/a/b/c/d/e': DIR,
    'deep/a/b/c/d/e/f.txt': fs.statSync('deep/a/b/c/d/e/f.txt').ino,
    foo: DIR,
    'foo/foo.txt': fs.statSync('foo/foo.txt').ino,
    'foo/bar.txt': fs.statSync('foo/bar.txt').ino,
    'foo_bar.txt': fs.statSync('foo_bar.txt').ino,
  } as InodeMap)

  for (const filePath of filePaths) {
    expect(statMethod).toHaveBeenCalledWith(filesMap.get(filePath))
  }
})

test('with provided stats', async () => {
  prepareEmpty()

  const startingIno = 7000
  const inoIncrement = 100
  const filePaths = [
    'deep/a/b/c/d/e/f.txt',
    'foo/foo.txt',
    'foo/bar.txt',
    'foo_bar.txt',
  ]
  const filesMap = new Map<string, string>()
  const filesStats: Record<string, ExtendFilesMapStats> = {}
  let ino = startingIno
  for (const filePath of filePaths) {
    filesMap.set(filePath, path.resolve(filePath))
    filesStats[filePath] = {
      ino,
      isDirectory: () => false,
      isFile: () => true,
    }
    ino += inoIncrement
  }

  const statMethod = mockFsPromiseStat()

  expect(await extendFilesMap({ filesMap, filesStats })).toStrictEqual({
    '.': DIR,
    deep: DIR,
    'deep/a': DIR,
    'deep/a/b': DIR,
    'deep/a/b/c': DIR,
    'deep/a/b/c/d': DIR,
    'deep/a/b/c/d/e': DIR,
    'deep/a/b/c/d/e/f.txt': startingIno,
    foo: DIR,
    'foo/foo.txt': startingIno + inoIncrement,
    'foo/bar.txt': startingIno + 2 * inoIncrement,
    'foo_bar.txt': startingIno + 3 * inoIncrement,
  } as InodeMap)

  expect(statMethod).not.toHaveBeenCalled()
})

test('skips inodes that are neither files nor directories', async () => {
  prepareEmpty()

  fs.mkdirSync('distribution', { recursive: true })
  fs.writeFileSync('distribution/index.js', '')

  const filesMap = new Map<string, string>([
    ['distribution/index.js', path.resolve('distribution/index.js')],
    ['.env', path.resolve('.env')],
  ])
  const filesStats: Record<string, ExtendFilesMapStats> = {
    'distribution/index.js': {
      ino: 7000,
      isDirectory: () => false,
      isFile: () => true,
    },
    // A FIFO — 1Password's environments create one for `.env`. It cannot be
    // hardlinked into the injected copy, so it belongs in neither map.
    '.env': {
      ino: 7100,
      isDirectory: () => false,
      isFile: () => false,
    },
  }

  expect(await extendFilesMap({ filesMap, filesStats })).toStrictEqual({
    '.': DIR,
    distribution: DIR,
    'distribution/index.js': 7000,
  } as InodeMap)
})

testOnPosix('skips a real FIFO', async () => {
  prepareEmpty()

  fs.mkdirSync('distribution', { recursive: true })
  fs.writeFileSync('distribution/index.js', '')
  execFileSync('mkfifo', [path.resolve('.env')])

  const filesMap = new Map<string, string>([
    ['distribution/index.js', path.resolve('distribution/index.js')],
    ['.env', path.resolve('.env')],
  ])

  expect(await extendFilesMap({ filesMap })).toStrictEqual({
    '.': DIR,
    distribution: DIR,
    'distribution/index.js': fs.statSync('distribution/index.js').ino,
  } as InodeMap)
})

testOnPosix('a skipped inode in the source removes what the target holds at that path, but a skipped inode in the target is left alone', async () => {
  prepareEmpty()

  fs.mkdirSync('source', { recursive: true })
  fs.mkdirSync('target', { recursive: true })
  fs.writeFileSync('source/keep.txt', '')
  fs.writeFileSync('target/keep.txt', '')
  // The source turned this path into a FIFO while the target still holds the
  // file that used to be there.
  execFileSync('mkfifo', [path.resolve('source/replaced.env')])
  fs.writeFileSync('target/replaced.env', 'stale')
  // A FIFO the target holds on its own.
  execFileSync('mkfifo', [path.resolve('target/own.env')])

  const patchers = await DirPatcher.fromMultipleTargets('source', ['target'])
  await Promise.all(patchers.map(async patcher => patcher.apply()))

  expect(fs.existsSync('target/replaced.env')).toBe(false)
  expect(fs.lstatSync('target/own.env').isFIFO()).toBe(true)
  expect(fs.existsSync('target/keep.txt')).toBe(true)
})

testOnPosix.each([
  ['a file', (sourcePath: string) => {
    fs.writeFileSync(sourcePath, 'real')
  }],
  ['a directory', (sourcePath: string) => {
    fs.mkdirSync(sourcePath, { recursive: true })
    fs.writeFileSync(path.join(sourcePath, 'inner.txt'), '')
  }],
])('replaces a skipped inode in the target when the source has %s there', async (_label, createSource) => {
  prepareEmpty()

  fs.mkdirSync('source', { recursive: true })
  fs.mkdirSync('target', { recursive: true })
  createSource(path.resolve('source/config.env'))
  fs.writeFileSync('source/other.txt', '')
  // The target holds an inode the map skips, so the diff cannot schedule it
  // for removal and adding over it would fail with EEXIST.
  execFileSync('mkfifo', [path.resolve('target/config.env')])

  const patchers = await DirPatcher.fromMultipleTargets('source', ['target'])
  await Promise.all(patchers.map(async patcher => patcher.apply()))

  const sourceStats = fs.lstatSync('source/config.env')
  const targetStats = fs.lstatSync('target/config.env')
  expect(targetStats.isFile()).toBe(sourceStats.isFile())
  expect(targetStats.isDirectory()).toBe(sourceStats.isDirectory())
  if (sourceStats.isDirectory()) {
    expect(fs.readdirSync('target/config.env')).toStrictEqual(fs.readdirSync('source/config.env'))
  }
  expect(fs.existsSync('target/other.txt')).toBe(true)
})

testOnPosix('keeps the files linked into a directory that replaced a blocking inode', async () => {
  prepareEmpty()

  fs.mkdirSync('source/blocked', { recursive: true })
  for (let index = 0; index < 20; index++) {
    fs.writeFileSync(`source/blocked/file${index}.txt`, '')
  }
  fs.mkdirSync('target', { recursive: true })
  execFileSync('mkfifo', [path.resolve('target/blocked')])

  // Widen the window between clearing the blocking inode and linking into the
  // directory that replaces it. Creating the directory concurrently with its
  // files lets a removal land after a sibling has linked, taking those files
  // with it.
  const originalRm = fs.promises.rm
  let delayNextRemoval = true
  fs.promises.rm = (async (target: fs.PathLike, options?: fs.RmOptions) => {
    if (delayNextRemoval) {
      delayNextRemoval = false
      await delay(30)
    }
    return originalRm(target, options)
  }) as typeof fs.promises.rm

  try {
    const patchers = await DirPatcher.fromMultipleTargets('source', ['target'])
    await Promise.all(patchers.map(async patcher => patcher.apply()))
  } finally {
    fs.promises.rm = originalRm
  }

  expect(fs.readdirSync('target/blocked').sort()).toStrictEqual(fs.readdirSync('source/blocked').sort())
})
