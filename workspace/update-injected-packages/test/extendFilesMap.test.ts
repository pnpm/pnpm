import fs from 'fs'
import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { type InodeMap, type ExtendFilesMapStats, DIR, extendFilesMap } from '../src/DirPatcher'

const originalStat = fs.promises.stat

function mockFsPromiseStat (): jest.Mock {
  const mockedMethod = jest.fn(fs.promises.stat)
  fs.promises.stat = mockedMethod as typeof fs.promises.stat
  return mockedMethod
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
  const filesIndex: Record<string, string> = {}
  for (const filePath of filePaths) {
    filesIndex[filePath] = path.resolve(filePath)
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
    fs.writeFileSync(filePath, '')
  }

  const statMethod = mockFsPromiseStat()

  expect(await extendFilesMap({ filesIndex })).toStrictEqual({
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
    expect(statMethod).toHaveBeenCalledWith(filesIndex[filePath])
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
  const filesIndex: Record<string, string> = {}
  const filesStats: Record<string, ExtendFilesMapStats> = {}
  let ino = startingIno
  for (const filePath of filePaths) {
    filesIndex[filePath] = path.resolve(filePath)
    filesStats[filePath] = {
      ino,
      isDirectory: () => false,
      isFile: () => true,
    }
    ino += inoIncrement
  }

  const statMethod = mockFsPromiseStat()

  expect(await extendFilesMap({ filesIndex, filesStats })).toStrictEqual({
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
