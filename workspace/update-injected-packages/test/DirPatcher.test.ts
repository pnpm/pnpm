import fs from 'fs'
import path from 'path'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { prepareEmpty } from '@pnpm/prepare'
import { DirPatcher } from '../src/DirPatcher'

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

test('optimally synchronizes source and target', async () => {
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

  const sourceDir = path.resolve('source')
  const targetDir = path.resolve('target')

  const sourceFetchResult = await fetchFromDir(sourceDir, { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultBefore = await fetchFromDir(targetDir, { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Object.keys(targetFetchResultBefore.filesIndex).sort()).not.toStrictEqual(Object.keys(sourceFetchResult.filesIndex).sort())
  expect(
    filesToModify
      .map(suffix => path.resolve(targetDir, suffix))
      .map(inodeNumber)
  ).not.toStrictEqual(
    filesToModify
      .map(suffix => path.resolve(sourceDir, suffix))
      .map(inodeNumber)
  )

  const patchers = await DirPatcher.fromMultipleTargets(sourceDir, [targetDir])
  expect(patchers).toMatchObject([{ sourceDir, targetDir }])

  const fsMethods = mockFsPromises()

  await patchers[0].apply()

  const targetFetchResultAfter = await fetchFromDir(targetDir, { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Object.keys(targetFetchResultAfter.filesIndex).sort()).toStrictEqual(Object.keys(sourceFetchResult.filesIndex).sort())
  expect(Object.keys(targetFetchResultAfter.filesIndex).sort()).not.toStrictEqual(Object.keys(targetFetchResultBefore.filesIndex).sort())
  expect(
    filesToModify
      .map(suffix => path.resolve(targetDir, suffix))
      .map(inodeNumber)
  ).toStrictEqual(
    filesToModify
      .map(suffix => path.resolve(sourceDir, suffix))
      .map(inodeNumber)
  )

  // does not touch filesToKeep
  for (const suffix of filesToKeep) {
    const sourceFile = path.resolve(sourceDir, suffix)
    const targetFile = path.resolve(targetDir, suffix)
    expect(fsMethods.rm).not.toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(sourceFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(expect.anything(), targetFile)
  }

  // remove filesToRemove without replacement
  for (const suffix of filesToRemove) {
    const sourceFile = path.resolve(sourceDir, suffix)
    const targetFile = path.resolve(targetDir, suffix)
    expect(fsMethods.rm).toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(sourceFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(expect.anything(), targetFile)
  }

  // add filesToAdd without removing old files
  for (const suffix of filesToAdd) {
    const sourceFile = path.resolve(sourceDir, suffix)
    const targetFile = path.resolve(targetDir, suffix)
    expect(fsMethods.rm).not.toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).toHaveBeenCalledWith(sourceFile, targetFile)
  }

  // replace filesToModify by removing old files and add new hardlinks
  for (const suffix of filesToModify) {
    const sourceFile = path.resolve(sourceDir, suffix)
    const targetFile = path.resolve(targetDir, suffix)
    expect(fsMethods.rm).toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).toHaveBeenCalledWith(sourceFile, targetFile)
  }

  expect(fsMethods.mkdir).toHaveBeenCalledWith(path.resolve(targetDir, 'files-to-add'), expect.anything())
  expect(fsMethods.mkdir).toHaveBeenCalledWith(path.resolve(targetDir, 'files-to-add/a'), expect.anything())
})
