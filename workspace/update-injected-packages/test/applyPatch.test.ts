import fs from 'fs'
import path from 'path'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { prepareEmpty } from '@pnpm/prepare'
import { type DirDiff, DIR, applyPatch } from '../src/DirPatcher'

const originalRm = fs.promises.rm
const originalMkdir = fs.promises.mkdir
const originalLink = fs.promises.link

function mockFsPromises (): Record<'rm' | 'mkdir' | 'link', jest.Mock> {
  const rm = jest.fn(fs.promises.rm)
  const mkdir = jest.fn(fs.promises.mkdir)
  const link = jest.fn(fs.promises.link)
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

const inodeNumber = (filePath: string): number => fs.lstatSync(filePath).ino

test('applies a patch on a directory', async () => {
  prepareEmpty()

  fs.mkdirSync('source')
  fs.mkdirSync('target')

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

  const optimizedDirPath: DirDiff = {
    added: [
      {
        path: 'files-to-add',
        newValue: DIR,
      },
      {
        path: 'files-to-add/a',
        newValue: DIR,
      },
      ...filesToAdd.map(path => ({ path, newValue: inodeNumber(`source/${path}`) })),
    ],
    removed: [
      {
        path: 'files-to-remove',
        oldValue: DIR,
      } as const,
      {
        path: 'files-to-remove/a',
        oldValue: DIR,
      } as const,
      ...filesToRemove.map(path => ({ path, oldValue: inodeNumber(`target/${path}`) })),
    ].reverse(),
    modified: [
      ...filesToModify.map(path => ({
        path,
        oldValue: inodeNumber(`target/${path}`),
        newValue: inodeNumber(`source/${path}`),
      })),
    ],
  }

  const sourceFetchResult = await fetchFromDir('source', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultBefore = await fetchFromDir('target', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Object.keys(targetFetchResultBefore.filesIndex).sort()).not.toStrictEqual(Object.keys(sourceFetchResult.filesIndex).sort())
  expect(
    filesToModify
      .map(suffix => `target/${suffix}`)
      .map(inodeNumber)
  ).not.toStrictEqual(
    filesToModify
      .map(suffix => `source/${suffix}`)
      .map(inodeNumber)
  )

  const fsMethods = mockFsPromises()

  await applyPatch(optimizedDirPath, path.resolve('source'), path.resolve('target'))

  const targetFetchResultAfter = await fetchFromDir('target', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Object.keys(targetFetchResultAfter.filesIndex).sort()).toStrictEqual(Object.keys(sourceFetchResult.filesIndex).sort())
  expect(Object.keys(targetFetchResultAfter.filesIndex).sort()).not.toStrictEqual(Object.keys(targetFetchResultBefore.filesIndex).sort())
  expect(
    filesToModify
      .map(suffix => `target/${suffix}`)
      .map(inodeNumber)
  ).toStrictEqual(
    filesToModify
      .map(suffix => `source/${suffix}`)
      .map(inodeNumber)
  )

  // does not touch filesToKeep
  for (const suffix of filesToKeep) {
    const sourceFile = path.resolve('source', suffix)
    const targetFile = path.resolve('target', suffix)
    expect(fsMethods.rm).not.toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(sourceFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(expect.anything(), targetFile)
  }

  // remove filesToRemove without replacement
  for (const suffix of filesToRemove) {
    const sourceFile = path.resolve('source', suffix)
    const targetFile = path.resolve('target', suffix)
    expect(fsMethods.rm).toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(sourceFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(expect.anything(), targetFile)
  }

  // add filesToAdd without removing old files
  for (const suffix of filesToAdd) {
    const sourceFile = path.resolve('source', suffix)
    const targetFile = path.resolve('target', suffix)
    expect(fsMethods.rm).not.toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).toHaveBeenCalledWith(sourceFile, targetFile)
  }

  // replace filesToModify by removing old files and add new hardlinks
  for (const suffix of filesToModify) {
    const sourceFile = path.resolve('source', suffix)
    const targetFile = path.resolve('target', suffix)
    expect(fsMethods.rm).toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).toHaveBeenCalledWith(sourceFile, targetFile)
  }

  expect(fsMethods.mkdir).toHaveBeenCalledWith(path.resolve('target', 'files-to-add'), expect.anything())
  expect(fsMethods.mkdir).toHaveBeenCalledWith(path.resolve('target', 'files-to-add/a'), expect.anything())
})
