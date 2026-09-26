// cspell:ignore mkfifo
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'

import { afterEach, expect, jest, test } from '@jest/globals'
import { fetchFromDir } from '@pnpm/fetching.directory-fetcher'
import { prepareEmpty } from '@pnpm/prepare'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import isWindows from 'is-windows'

import { DirPatcher } from '../src/DirPatcher.js'

// `mkfifo` has no Windows equivalent, and neither has any other inode type
// that `extendFilesMap` skips.
const testOnPosix = isWindows() ? test.skip : test

const originalRm = fs.promises.rm
const originalMkdir = fs.promises.mkdir
const originalLink = fs.promises.link

function mockFsPromises (): Record<'rm' | 'mkdir' | 'link', jest.Mock> {
  const rm = jest.fn(fs.promises.rm) as jest.Mock
  const mkdir = jest.fn(fs.promises.mkdir) as jest.Mock
  const link = jest.fn(fs.promises.link) as jest.Mock
  fs.promises.rm = rm as typeof fs.promises.rm
  fs.promises.mkdir = mkdir as typeof fs.promises.mkdir
  fs.promises.link = link as typeof fs.promises.link
  return { rm, mkdir, link }
}

function restoreAllMocks (): void {
  jest.resetAllMocks()
  fs.promises.rm = originalRm
  fs.promises.mkdir = originalMkdir
  fs.promises.link = originalLink
}

afterEach(restoreAllMocks)

function createDir (dirPath: string): void {
  fs.mkdirSync(dirPath, { recursive: true })
}

function createFile (filePath: string, content: string = ''): void {
  createDir(path.dirname(filePath))
  fs.writeFileSync(filePath, content)
}

function createHardlink (existingPath: string, newPath: string): void {
  createDir(path.dirname(newPath))
  fs.linkSync(existingPath, newPath)
}

/** Stands in for every inode type `extendFilesMap` skips. */
function createFifo (fifoPath: string): void {
  createDir(path.dirname(fifoPath))
  execFileSync('mkfifo', [path.resolve(fifoPath)])
}

const fileId = (filePath: string): string => {
  const stats = fs.lstatSync(filePath)
  return `${stats.dev}:${stats.ino}`
}

test('optimally synchronizes source and target', async () => {
  prepareEmpty()

  createDir('source')
  createDir('target')

  /** Same files that exist in both source and target */
  const filesToKeep = [
    'files-to-keep/a/a.txt',
    'files-to-keep/a/b.txt',
    'files-to-keep/b.txt',
    'single-file-to-keep.txt',
  ] as const
  for (const suffix of filesToKeep) {
    const source = `source/${suffix}`
    const target = `target/${suffix}`
    createFile(source, '')
    createHardlink(source, target)
  }

  /** Files that no longer exist in source but still exist in target */
  const filesToRemove = [
    'files-to-remove/a/a.txt',
    'files-to-remove/a/b.txt',
    'files-to-remove/b.txt',
    'single-file-to-remove.txt',
  ] as const
  for (const suffix of filesToRemove) {
    createFile(`target/${suffix}`)
  }

  /** Files that exist in source but not yet in target */
  const filesToAdd = [
    'files-to-add/a/a.txt',
    'files-to-add/a/b.txt',
    'files-to-add/b.txt',
    'single-file-to-add.txt',
  ] as const
  for (const suffix of filesToAdd) {
    createFile(`source/${suffix}`)
  }

  /** Unequal files that exist in both source and target */
  const filesToModify = [
    'files-to-modify/a/a.txt',
    'files-to-modify/a/b.txt',
    'files-to-modify/b.txt',
    'single-file-to-modify.txt',
  ] as const
  for (const suffix of filesToModify) {
    createFile(`source/${suffix}`, 'new content')
    createFile(`target/${suffix}`, 'old content')
  }

  const sourceDir = path.resolve('source')
  const targetDir = path.resolve('target')

  const sourceFetchResult = await fetchFromDir(sourceDir, { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultBefore = await fetchFromDir(targetDir, { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Array.from(targetFetchResultBefore.filesMap.keys()).sort(lexCompare)).not.toStrictEqual(Array.from(sourceFetchResult.filesMap.keys()).sort(lexCompare))
  expect(
    filesToModify
      .map(suffix => path.resolve(targetDir, suffix))
      .map(fileId)
  ).not.toStrictEqual(
    filesToModify
      .map(suffix => path.resolve(sourceDir, suffix))
      .map(fileId)
  )

  let fsMethods = mockFsPromises()

  const patchers = await DirPatcher.fromMultipleTargets(sourceDir, [targetDir])
  expect(patchers).toMatchObject([{ sourceDir, targetDir }])
  expect(fsMethods.rm).not.toHaveBeenCalled()
  expect(fsMethods.mkdir).not.toHaveBeenCalled()
  expect(fsMethods.link).not.toHaveBeenCalled()

  restoreAllMocks()
  fsMethods = mockFsPromises()

  await patchers[0].apply()

  const targetFetchResultAfter = await fetchFromDir(targetDir, { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Array.from(targetFetchResultAfter.filesMap.keys()).sort(lexCompare)).toStrictEqual(Array.from(sourceFetchResult.filesMap.keys()).sort(lexCompare))
  expect(Array.from(targetFetchResultAfter.filesMap.keys()).sort(lexCompare)).not.toStrictEqual(Array.from(targetFetchResultBefore.filesMap.keys()).sort(lexCompare))
  expect(
    filesToModify
      .map(suffix => path.resolve(targetDir, suffix))
      .map(fileId)
  ).toStrictEqual(
    filesToModify
      .map(suffix => path.resolve(sourceDir, suffix))
      .map(fileId)
  )

  // does not touch filesToKeep
  for (const suffix of filesToKeep) {
    const sourceFile = path.resolve(sourceDir, suffix)
    const targetFile = path.resolve(targetDir, suffix)
    expect(fsMethods.rm).not.toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(sourceFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(expect.anything(), targetFile)
  }

  // removes filesToRemove without replacement
  for (const suffix of filesToRemove) {
    const sourceFile = path.resolve(sourceDir, suffix)
    const targetFile = path.resolve(targetDir, suffix)
    expect(fsMethods.rm).toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(sourceFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(expect.anything(), targetFile)
  }

  // adds filesToAdd without removing old files
  for (const suffix of filesToAdd) {
    const sourceFile = path.resolve(sourceDir, suffix)
    const targetFile = path.resolve(targetDir, suffix)
    expect(fsMethods.rm).not.toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).toHaveBeenCalledWith(sourceFile, targetFile)
  }

  // replaces filesToModify by removing old files and add new hardlinks
  for (const suffix of filesToModify) {
    const sourceFile = path.resolve(sourceDir, suffix)
    const targetFile = path.resolve(targetDir, suffix)
    expect(fsMethods.rm).toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).toHaveBeenCalledWith(sourceFile, targetFile)
  }

  expect(fsMethods.mkdir).toHaveBeenCalledWith(path.resolve(targetDir, 'files-to-add'), expect.anything())
  expect(fsMethods.mkdir).toHaveBeenCalledWith(path.resolve(targetDir, 'files-to-add/a'), expect.anything())
})

test('multiple patchers', async () => {
  prepareEmpty()

  createDir('target1')
  createDir('target2')
  createDir('target3')

  createFile('source/dir/file1.txt')
  createFile('source/dir/file2.txt')
  createFile('source/file3.txt')

  const patchers = await DirPatcher.fromMultipleTargets('source', ['target1', 'target2', 'target3'])
  expect(patchers).toMatchObject([
    { sourceDir: 'source', targetDir: 'target1' },
    { sourceDir: 'source', targetDir: 'target2' },
    { sourceDir: 'source', targetDir: 'target3' },
  ])

  const sourceFetchResult = await fetchFromDir('source', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultBefore1 = await fetchFromDir('target1', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultBefore2 = await fetchFromDir('target2', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultBefore3 = await fetchFromDir('target3', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const expected = Array.from(sourceFetchResult.filesMap.keys()).sort(lexCompare)
  expect(Array.from(targetFetchResultBefore1.filesMap.keys()).sort(lexCompare)).not.toStrictEqual(expected)
  expect(Array.from(targetFetchResultBefore2.filesMap.keys()).sort(lexCompare)).not.toStrictEqual(expected)
  expect(Array.from(targetFetchResultBefore3.filesMap.keys()).sort(lexCompare)).not.toStrictEqual(expected)
  expect(Array.from(targetFetchResultBefore1.filesMap.keys()).sort(lexCompare)).toStrictEqual([])
  expect(Array.from(targetFetchResultBefore2.filesMap.keys()).sort(lexCompare)).toStrictEqual([])
  expect(Array.from(targetFetchResultBefore3.filesMap.keys()).sort(lexCompare)).toStrictEqual([])

  await Promise.all(patchers.map(patcher => patcher.apply()))

  const targetFetchResultAfter1 = await fetchFromDir('target1', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultAfter2 = await fetchFromDir('target2', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultAfter3 = await fetchFromDir('target3', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Array.from(targetFetchResultAfter1.filesMap.keys()).sort(lexCompare)).toStrictEqual(expected)
  expect(Array.from(targetFetchResultAfter2.filesMap.keys()).sort(lexCompare)).toStrictEqual(expected)
  expect(Array.from(targetFetchResultAfter3.filesMap.keys()).sort(lexCompare)).toStrictEqual(expected)
})

test('replaces a target entry whose inode type changed in the source', async () => {
  prepareEmpty()

  createFile('source/became-a-dir/index.js', 'inner')
  createFile('source/became-a-file', 'now a file')
  createFile('target/became-a-dir', 'was a file')
  createFile('target/became-a-file/index.js', 'was a dir')

  const patchers = await DirPatcher.fromMultipleTargets('source', ['target'])
  await Promise.all(patchers.map(async patcher => patcher.apply()))

  expect(fs.readFileSync('target/became-a-dir/index.js', 'utf8')).toBe('inner')
  expect(fs.readFileSync('target/became-a-file', 'utf8')).toBe('now a file')
})

testOnPosix('removes what the target holds where the source has a skipped inode, but leaves a skipped inode of its own alone', async () => {
  prepareEmpty()

  createFile('source/keep.txt')
  createFile('target/keep.txt')
  // The source turned this path into a FIFO while the target still holds the
  // file that used to be there.
  createFifo('source/replaced.env')
  createFile('target/replaced.env', 'stale')
  // A FIFO the target holds on its own.
  createFifo('target/own.env')

  const patchers = await DirPatcher.fromMultipleTargets('source', ['target'])
  await Promise.all(patchers.map(async patcher => patcher.apply()))

  expect(fs.existsSync('target/replaced.env')).toBe(false)
  expect(fs.lstatSync('target/own.env').isFIFO()).toBe(true)
  expect(fs.existsSync('target/keep.txt')).toBe(true)
})

testOnPosix.each([
  ['a file', (sourcePath: string) => {
    createFile(sourcePath, 'real')
  }],
  ['a directory', (sourcePath: string) => {
    createFile(path.join(sourcePath, 'inner.txt'))
  }],
])('replaces a skipped inode in the target when the source has %s there', async (_label, createSource) => {
  prepareEmpty()

  createSource('source/config.env')
  createFile('source/other.txt')
  // The target holds an inode the map skips, so the diff cannot schedule it
  // for removal and adding over it would fail with EEXIST.
  createFifo('target/config.env')

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

  const fileNames = Array.from({ length: 20 }, (_, index) => `file${index}.txt`)
  for (const fileName of fileNames) {
    createFile(`source/blocked/${fileName}`)
  }
  createDir('target')
  createFifo('target/blocked')

  // Widen the window between clearing the blocking inode and linking into the
  // directory that replaces it. Were the directory created concurrently with
  // its files, this removal would land after a sibling had linked and take
  // those files with it.
  let delayNextRemoval = true
  fs.promises.rm = (async (target: fs.PathLike, options?: fs.RmOptions) => {
    if (delayNextRemoval) {
      delayNextRemoval = false
      await delay(30)
    }
    return originalRm(target, options)
  }) as typeof fs.promises.rm

  const patchers = await DirPatcher.fromMultipleTargets('source', ['target'])
  await Promise.all(patchers.map(async patcher => patcher.apply()))

  expect(fs.readdirSync('target/blocked').sort()).toStrictEqual(fileNames.sort())
})

test('removes what the source dropped before replacing the directory that held it', async () => {
  prepareEmpty()

  createFile('source/became-a-file', 'now a file')
  createFile('target/became-a-file/dropped.txt', 'was a dir')

  // `became-a-file` is a modification, `became-a-file/dropped.txt` a
  // removal. Hold the removal back so it would land after the
  // modification has linked a file over its parent — at which point
  // the path it was given no longer has a directory in it.
  let delayNextRemoval = true
  fs.promises.rm = (async (target: fs.PathLike, options?: fs.RmOptions) => {
    if (delayNextRemoval && String(target).endsWith('dropped.txt')) {
      delayNextRemoval = false
      await delay(30)
    }
    return originalRm(target, options)
  }) as typeof fs.promises.rm

  const patchers = await DirPatcher.fromMultipleTargets('source', ['target'])
  await Promise.all(patchers.map(async patcher => patcher.apply()))

  expect(fs.readFileSync('target/became-a-file', 'utf8')).toBe('now a file')
})
