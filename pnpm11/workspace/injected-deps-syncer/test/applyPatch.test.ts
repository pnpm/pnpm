import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import { fetchFromDir } from '@pnpm/fetching.directory-fetcher'
import { prepareEmpty } from '@pnpm/prepare'
import { lexCompare } from '@pnpm/text.ordinal-comparator'

import { applyPatch, DIR, type DirDiff, DirPatcher, publishEditsForWatchers, readPublishSource } from '../src/DirPatcher.js'

const originalRm = fs.promises.rm
const originalMkdir = fs.promises.mkdir
const originalLink = fs.promises.link
const originalCopyFile = fs.promises.copyFile

test('waits for child removal before recursively removing its parent', async () => {
  const targetDir = path.resolve('target')
  const child = path.join(targetDir, 'removed', 'child')
  const parent = path.join(targetDir, 'removed')
  const completed: string[] = []
  let removingChild = false
  fs.promises.rm = jest.fn<typeof fs.promises.rm>(async targetPath => {
    const target = String(targetPath)
    if (target === parent && removingChild) {
      throw Object.assign(new Error('overlapping recursive removal'), { code: 'EPERM' })
    }
    removingChild = target === child
    await Promise.resolve()
    completed.push(target)
    removingChild = false
  })

  await applyPatch({
    added: [],
    modified: [],
    removed: [
      { path: path.join('removed', 'child'), oldValue: DIR },
      { path: 'removed', oldValue: DIR },
    ],
  }, path.resolve('source'), targetDir)

  expect(completed).toStrictEqual([child, parent])
})

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
  fs.promises.copyFile = originalCopyFile
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

const fileId = (filePath: string): string => {
  const stats = fs.lstatSync(filePath)
  return `${stats.dev}:${stats.ino}`
}

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
      ...filesToAdd.map(path => ({ path, newValue: fileId(`source/${path}`) })),
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
      ...filesToRemove.map(path => ({ path, oldValue: fileId(`target/${path}`) })),
    ].reverse(),
    modified: [
      ...filesToModify.map(path => ({
        path,
        oldValue: fileId(`target/${path}`),
        newValue: fileId(`source/${path}`),
      })),
    ],
  }

  const sourceFetchResult = await fetchFromDir('source', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  const targetFetchResultBefore = await fetchFromDir('target', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Array.from(targetFetchResultBefore.filesMap.keys()).sort(lexCompare)).not.toStrictEqual(Array.from(sourceFetchResult.filesMap.keys()).sort(lexCompare))
  expect(
    filesToModify
      .map(suffix => `target/${suffix}`)
      .map(fileId)
  ).not.toStrictEqual(
    filesToModify
      .map(suffix => `source/${suffix}`)
      .map(fileId)
  )

  const fsMethods = mockFsPromises()

  await applyPatch(optimizedDirPath, path.resolve('source'), path.resolve('target'))

  const targetFetchResultAfter = await fetchFromDir('target', { includeOnlyPackageFiles: false, resolveSymlinks: true })
  expect(Array.from(targetFetchResultAfter.filesMap.keys()).sort(lexCompare)).toStrictEqual(Array.from(sourceFetchResult.filesMap.keys()).sort(lexCompare))
  expect(Array.from(targetFetchResultAfter.filesMap.keys()).sort(lexCompare)).not.toStrictEqual(Array.from(targetFetchResultBefore.filesMap.keys()).sort(lexCompare))
  expect(
    filesToModify
      .map(suffix => `target/${suffix}`)
      .map(fileId)
  ).toStrictEqual(
    filesToModify
      .map(suffix => `source/${suffix}`)
      .map(fileId)
  )

  for (const suffix of filesToKeep) {
    const sourceFile = path.resolve('source', suffix)
    const targetFile = path.resolve('target', suffix)
    expect(fsMethods.rm).not.toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(sourceFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(expect.anything(), targetFile)
  }

  for (const suffix of filesToRemove) {
    const sourceFile = path.resolve('source', suffix)
    const targetFile = path.resolve('target', suffix)
    expect(fsMethods.rm).toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(sourceFile, expect.anything())
    expect(fsMethods.link).not.toHaveBeenCalledWith(expect.anything(), targetFile)
  }

  for (const suffix of filesToAdd) {
    const sourceFile = path.resolve('source', suffix)
    const targetFile = path.resolve('target', suffix)
    expect(fsMethods.rm).not.toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).toHaveBeenCalledWith(sourceFile, targetFile)
  }

  for (const suffix of filesToModify) {
    const sourceFile = path.resolve('source', suffix)
    const targetFile = path.resolve('target', suffix)
    expect(fsMethods.rm).toHaveBeenCalledWith(targetFile, expect.anything())
    expect(fsMethods.link).toHaveBeenCalledWith(sourceFile, targetFile)
  }

  expect(fsMethods.mkdir).toHaveBeenCalledWith(path.resolve('target', 'files-to-add'), expect.anything())
  expect(fsMethods.mkdir).toHaveBeenCalledWith(path.resolve('target', 'files-to-add/a'), expect.anything())
})

test('falls back to copy when link fails with EXDEV', async () => {
  prepareEmpty()

  createFile('source/file.txt', 'hello world')
  createDir('target')

  fs.promises.link = jest.fn<typeof fs.promises.link>(async () => {
    throw Object.assign(new Error('cross-device link not permitted'), { code: 'EXDEV' })
  })

  await applyPatch({
    added: [
      { path: 'file.txt', newValue: 'source/file.txt' },
    ],
    removed: [],
    modified: [],
  }, path.resolve('source'), path.resolve('target'))

  expect(fs.readFileSync('target/file.txt', 'utf8')).toBe('hello world')
  expect(fs.readdirSync('target')).toStrictEqual(['file.txt'])
})

// Creating a file symlink needs extra privileges on Windows.
const testOnPosix = process.platform === 'win32' ? test.skip : test

testOnPosix('does not copy through a symlink that occupies the target when link fails with EXDEV', async () => {
  prepareEmpty()

  createFile('source/file.txt', 'hello world')
  createFile('victim.txt', 'untouched')
  createDir('target')
  fs.symlinkSync(path.resolve('victim.txt'), path.resolve('target/file.txt'))

  fs.promises.link = jest.fn<typeof fs.promises.link>(async () => {
    throw Object.assign(new Error('cross-device link not permitted'), { code: 'EXDEV' })
  })

  await applyPatch({
    added: [
      { path: 'file.txt', newValue: 'source/file.txt' },
    ],
    removed: [],
    modified: [],
  }, path.resolve('source'), path.resolve('target'))

  expect(fs.readFileSync('victim.txt', 'utf8')).toBe('untouched')
  expect(fs.lstatSync('target/file.txt').isSymbolicLink()).toBe(false)
  expect(fs.readFileSync('target/file.txt', 'utf8')).toBe('hello world')
})

function fileIdentity (filePath: string): string {
  const stats = fs.statSync(filePath)
  return `${stats.dev}:${stats.ino}`
}

test('publishEditsForWatchers replaces an edited hardlink with a file a watcher can see', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'injected-publish-'))
  const source = path.join(root, 'source')
  const target = path.join(root, 'target')
  const sourceFile = path.join(source, 'index.js')
  const targetFile = path.join(target, 'index.js')
  createFile(sourceFile, 'old')
  createHardlink(sourceFile, targetFile)
  fs.writeFileSync(sourceFile, 'new')

  const events: string[] = []
  const watcher = fs.watch(target, (_event, filename) => {
    events.push(String(filename))
  })
  try {
    await publishEditsForWatchers(await readPublishSource(source), target, 0)
    await new Promise<void>(resolve => {
      setTimeout(resolve, 200)
    })
    expect(fs.readFileSync(targetFile, 'utf8')).toBe('new')
    expect(fileIdentity(targetFile)).not.toBe(fileIdentity(sourceFile))
    expect(events.length).toBeGreaterThan(0)
  } finally {
    watcher.close()
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('publishEditsForWatchers leaves out the files the manifest does not list when reading package files', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'injected-publish-'))
  const source = path.join(root, 'source')
  const target = path.join(root, 'target')
  createFile(path.join(source, 'package.json'), JSON.stringify({ name: 'source', files: ['index.js'] }))
  createFile(path.join(source, 'index.js'), 'built')
  createFile(path.join(source, 'src/index.ts'), 'source')
  fs.mkdirSync(target)
  try {
    await publishEditsForWatchers(await readPublishSource(source, true), target, 0)
    expect(fs.readFileSync(path.join(target, 'index.js'), 'utf8')).toBe('built')
    expect(fs.existsSync(path.join(target, 'src'))).toBe(false)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

testOnPosix('readPublishSource skips a dangling symlink among the package files', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'injected-publish-'))
  const source = path.join(root, 'source')
  createFile(path.join(source, 'package.json'), JSON.stringify({ name: 'source' }))
  createFile(path.join(source, 'index.js'), 'built')
  fs.symlinkSync('missing.js', path.join(source, 'dangling.js'))
  try {
    const { map } = await readPublishSource(source, true)
    expect(Object.keys(map).sort()).toStrictEqual(['.', 'index.js', 'package.json'])
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

function prepareSourceWithLinkedDirectory (root: string, files: string[]): string {
  const source = path.join(root, 'source')
  createFile(path.join(source, 'package.json'), JSON.stringify({ name: 'source', files }))
  createFile(path.join(source, 'files/foo/bar.txt'), 'bar')
  fs.symlinkSync('files/foo', path.join(source, 'link-to-a-dir'))
  return source
}

// The install keeps a package's link to its own content as a link, and
// leaves out a link to a directory it does not import.
testOnPosix('readPublishSource keeps a link to a listed directory as a link', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'injected-publish-'))
  try {
    const { map } = await readPublishSource(prepareSourceWithLinkedDirectory(root, ['link-to-a-dir', 'files']), true)
    expect(map['link-to-a-dir']).toBe('link:files/foo')
    expect(map['files/foo/bar.txt']).toEqual(expect.any(String))
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

testOnPosix('a sync copies a link to a file the packlist does not list as the file', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'injected-publish-'))
  try {
    const source = path.join(root, 'source')
    const target = path.join(root, 'target')
    createFile(path.join(source, 'package.json'), JSON.stringify({ name: 'source', files: ['link.txt'] }))
    createFile(path.join(source, 'src/real.txt'), 'real')
    fs.symlinkSync('src/real.txt', path.join(source, 'link.txt'))
    fs.mkdirSync(target)

    const patchers = await DirPatcher.fromMultipleTargets(source, [target], true)
    await Promise.all(patchers.map(async (patcher) => patcher.apply()))

    expect(fs.lstatSync(path.join(target, 'link.txt')).isFile()).toBe(true)
    expect(fs.readFileSync(path.join(target, 'link.txt'), 'utf8')).toBe('real')
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

testOnPosix('readPublishSource leaves out a link to a directory the packlist does not list', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'injected-publish-'))
  try {
    const { map } = await readPublishSource(prepareSourceWithLinkedDirectory(root, ['link-to-a-dir']), true)
    expect(Object.keys(map).filter((key) => key.startsWith('link-to-a-dir'))).toEqual([])
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('publishEditsForWatchers leaves a hardlink that predates the watch', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'injected-publish-'))
  const source = path.join(root, 'source')
  const target = path.join(root, 'target')
  const sourceFile = path.join(source, 'index.js')
  const targetFile = path.join(target, 'index.js')
  createFile(sourceFile, 'same')
  createHardlink(sourceFile, targetFile)

  try {
    await publishEditsForWatchers(await readPublishSource(source), target, Date.now() + 86_400_000)
    expect(fileIdentity(targetFile)).toBe(fileIdentity(sourceFile))
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

testOnPosix('publishEditsForWatchers keeps the mode of the source file', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'injected-publish-'))
  const source = path.join(root, 'source')
  const target = path.join(root, 'target')
  const sourceFile = path.join(source, 'bin.js')
  createFile(sourceFile, 'old')
  createHardlink(sourceFile, path.join(target, 'bin.js'))
  fs.writeFileSync(sourceFile, 'new')
  fs.chmodSync(sourceFile, 0o755)

  try {
    await publishEditsForWatchers(await readPublishSource(source), target, 0)
    expect(fs.statSync(path.join(target, 'bin.js')).mode & 0o777).toBe(0o755)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})
