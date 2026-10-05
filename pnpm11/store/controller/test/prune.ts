import { promises as fs } from 'node:fs'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import { StoreIndex } from '@pnpm/store.index'
import { temporaryDirectory } from 'tempy'

import { prune } from '../src/storeController/prune.js'

const temporaryDirectories: string[] = []
const storeIndexes: StoreIndex[] = []

afterEach(async () => {
  jest.restoreAllMocks()
  for (const storeIndex of storeIndexes.splice(0)) storeIndex.close()
  await Promise.all(temporaryDirectories.splice(0).map(directory => fs.rm(directory, { recursive: true, force: true })))
})

test('prune removes stale spill directories and keeps recent directories and unrelated entries', async () => {
  const options = createPruneOptions()
  const oldDate = new Date(Date.now() - 48 * 60 * 60_000)
  await Promise.all(['download-', 'tarball-'].map(async (prefix) => {
    const staleDirectory = await fs.mkdtemp(path.join(options.storeDir, prefix))
    await fs.writeFile(path.join(staleDirectory, 'archive'), 'leftover data')
    await fs.utimes(path.join(staleDirectory, 'archive'), oldDate, oldDate)
    await fs.utimes(staleDirectory, oldDate, oldDate)
    const recentDirectory = await fs.mkdtemp(path.join(options.storeDir, prefix))
    await fs.writeFile(path.join(recentDirectory, 'archive'), 'active data')
  }))
  const unrelatedDirectory = path.join(options.storeDir, 'unrelated')
  await fs.mkdir(unrelatedDirectory)
  await fs.utimes(unrelatedDirectory, oldDate, oldDate)
  const unrelatedFile = path.join(options.storeDir, 'download-file')
  await fs.writeFile(unrelatedFile, 'keep')
  await fs.utimes(unrelatedFile, oldDate, oldDate)

  await prune(options)

  const entries = await fs.readdir(options.storeDir)
  expect(entries.filter(entry => entry.startsWith('download-') && entry !== 'download-file')).toHaveLength(1)
  expect(entries.filter(entry => entry.startsWith('tarball-'))).toHaveLength(1)
  expect(await fs.readFile(unrelatedFile, 'utf8')).toBe('keep')
  expect((await fs.stat(unrelatedDirectory)).isDirectory()).toBe(true)
  await Promise.all(entries.filter(entry => entry.startsWith('tarball-') || (entry.startsWith('download-') && entry !== 'download-file')).map(async (entry) => {
    expect(await fs.readFile(path.join(options.storeDir, entry, 'archive'), 'utf8')).toBe('active data')
  }))
})

test('prune keeps old spill directories whose files were written recently', async () => {
  const options = createPruneOptions()
  const oldDate = new Date(Date.now() - 48 * 60 * 60_000)
  const directories = await Promise.all(['download-', 'tarball-'].map(async (prefix) => {
    const directory = await fs.mkdtemp(path.join(options.storeDir, prefix))
    await fs.writeFile(path.join(directory, 'archive'), 'active download')
    await fs.utimes(directory, oldDate, oldDate)
    return directory
  }))
  await prune(options)
  await Promise.all(directories.map(async (directory) => {
    expect(await fs.readFile(path.join(directory, 'archive'), 'utf8')).toBe('active download')
  }))
})

test('prune stops scanning a spill directory after finding a recent file', async () => {
  const options = createPruneOptions()
  const directory = await fs.mkdtemp(path.join(options.storeDir, 'download-'))
  await Promise.all(['first', 'second'].map(file => fs.writeFile(path.join(directory, file), 'keep')))
  const stat = jest.spyOn(fs, 'lstat')
  const oldDate = new Date(Date.now() - 48 * 60 * 60_000)
  await fs.utimes(directory, oldDate, oldDate)

  await prune(options)

  expect(stat).toHaveBeenCalledTimes(2)
  expect(stat).toHaveBeenNthCalledWith(1, directory)
  expect(await fs.readdir(directory)).toHaveLength(2)
})

test('prune removes empty stale spill directories', async () => {
  const options = createPruneOptions()
  const directory = await fs.mkdtemp(path.join(options.storeDir, 'tarball-'))
  const oldDate = new Date(Date.now() - 48 * 60 * 60_000)
  await fs.utimes(directory, oldDate, oldDate)
  await prune(options)
  await expect(fs.lstat(directory)).rejects.toMatchObject({ code: 'ENOENT' })
})

test('prune tolerates a missing store directory', async () => {
  const options = createPruneOptions()
  options.storeDir = path.join(options.storeDir, 'missing')
  await expect(prune(options)).resolves.toBeUndefined()
})

test('prune does not follow spill directory symlinks', async () => {
  const options = createPruneOptions()
  const target = path.join(options.cacheDir, 'target')
  await fs.mkdir(target, { recursive: true })
  await fs.writeFile(path.join(target, 'archive'), 'keep')
  const oldDate = new Date(Date.now() - 48 * 60 * 60_000)
  await fs.utimes(target, oldDate, oldDate)
  await Promise.all(['download-', 'tarball-'].map(async (prefix) => {
    await fs.symlink(target, path.join(options.storeDir, `${prefix}link`), 'junction')
  }))

  await prune(options)

  expect(await fs.readFile(path.join(target, 'archive'), 'utf8')).toBe('keep')
  expect((await fs.lstat(path.join(options.storeDir, 'download-link'))).isSymbolicLink()).toBe(true)
  expect((await fs.lstat(path.join(options.storeDir, 'tarball-link'))).isSymbolicLink()).toBe(true)
})

test('prune tolerates a spill directory removed by its owner during cleanup', async () => {
  const options = createPruneOptions()
  await fs.mkdir(path.join(options.storeDir, 'download-disappeared'))
  jest.spyOn(fs, 'lstat').mockRejectedValueOnce(Object.assign(new Error('already removed'), { code: 'ENOENT' }))
  await expect(prune(options)).resolves.toBeUndefined()
})

test('prune propagates unexpected spill directory errors', async () => {
  const options = createPruneOptions()
  await fs.mkdir(path.join(options.storeDir, 'tarball-denied'))
  const error = Object.assign(new Error('access denied'), { code: 'EACCES' })
  jest.spyOn(fs, 'lstat').mockRejectedValueOnce(error)
  await expect(prune(options)).rejects.toBe(error)
})

function createPruneOptions () {
  const directory = temporaryDirectory()
  temporaryDirectories.push(directory)
  const storeDir = path.join(directory, 'store')
  const storeIndex = new StoreIndex(storeDir)
  storeIndexes.push(storeIndex)
  return { storeDir, cacheDir: path.join(directory, 'cache'), storeIndex }
}
