import fs from 'fs'
import path from 'path'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { prepareEmpty } from '@pnpm/prepare'
import { type DirDiff, DIR, applyPatch } from '../src/DirPatcher'

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
  const filesToDelete = [
    'files-to-delete/a/a.txt',
    'files-to-delete/a/b.txt',
    'files-to-delete/b.txt',
    'single-file-to-delete.txt',
  ] as const
  for (const suffix of filesToDelete) {
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
  const filesToChange = [
    'files-to-keep/a/a.txt',
    'files-to-keep/a/b.txt',
    'files-to-keep/b.txt',
    'single-file-to-keep.txt',
  ] as const
  for (const suffix of filesToChange) {
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
        path: 'files-to-removed',
        oldValue: DIR,
      } as const,
      {
        path: 'files-to-removed/a',
        oldValue: DIR,
      } as const,
      ...filesToDelete.map(path => ({ path, oldValue: inodeNumber(`target/${path}`) })),
    ].reverse(),
    modified: [
      ...filesToChange.map(path => ({
        path,
        oldValue: inodeNumber(`target/${path}`),
        newValue: inodeNumber(`source/${path}`),
      })),
    ],
  }

  const sourceFetchResult = await fetchFromDir('source', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultBefore = await fetchFromDir('target', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Object.keys(targetFetchResultBefore.filesIndex).sort()).not.toStrictEqual(Object.keys(sourceFetchResult.filesIndex).sort())

  await applyPatch(optimizedDirPath, path.resolve('source'), path.resolve('target'))

  const targetFetchResultAfter = await fetchFromDir('target', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Object.keys(targetFetchResultAfter.filesIndex).sort()).toStrictEqual(Object.keys(sourceFetchResult.filesIndex).sort())
  expect(Object.keys(targetFetchResultAfter.filesIndex).sort()).not.toStrictEqual(Object.keys(targetFetchResultBefore.filesIndex).sort())
})
