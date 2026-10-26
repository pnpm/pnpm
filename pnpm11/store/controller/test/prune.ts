import { existsSync, promises as fs } from 'node:fs'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import { getFilePathByModeInCafs } from '@pnpm/store.cafs'
import { StoreIndex } from '@pnpm/store.index'
import { temporaryDirectory } from 'tempy'

import { registerProject } from '../src/storeController/projectRegistry.js'
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

test('prune keeps stores when the cache directory is their parent', async () => {
  const directory = temporaryDirectory()
  temporaryDirectories.push(directory)
  const cacheDir = directory
  const storeDir = path.join(cacheDir, 'v12', 'metadata-store')
  const storeIndex = new StoreIndex(storeDir)
  storeIndexes.push(storeIndex)
  await fs.mkdir(path.join(storeDir, 'files', '00'), { recursive: true })
  await fs.mkdir(path.join(cacheDir, 'v12', 'metadata-cache'), { recursive: true })
  await fs.mkdir(path.join(cacheDir, 'v11', 'files', '00'), { recursive: true })
  await fs.mkdir(path.join(cacheDir, 'v11', 'metadata-cache'), { recursive: true })

  await prune({ cacheDir, storeDir, storeIndex })

  expect(await fs.stat(storeDir)).toBeTruthy()
  expect(await fs.stat(path.join(cacheDir, 'v11', 'files', '00'))).toBeTruthy()
  await expect(fs.stat(path.join(cacheDir, 'v12', 'metadata-cache'))).rejects.toMatchObject({ code: 'ENOENT' })
  await expect(fs.stat(path.join(cacheDir, 'v11', 'metadata-cache'))).rejects.toMatchObject({ code: 'ENOENT' })
})

test('prune keeps protected global, home, and state directories', async () => {
  const options = createPruneOptions()
  const protectedDirs = [
    path.join(options.cacheDir, 'metadata-home'),
    path.join(options.cacheDir, 'v11', 'metadata-global'),
    path.join(options.cacheDir, 'metadata-state'),
  ]
  await Promise.all(protectedDirs.map(directory => fs.mkdir(path.join(directory, 'keep'), { recursive: true })))
  await fs.mkdir(path.join(options.cacheDir, 'v11', 'metadata-cache'), { recursive: true })

  await prune({ ...options, protectedDirs })

  await Promise.all(protectedDirs.map(async directory => {
    expect(await fs.stat(path.join(directory, 'keep'))).toBeTruthy()
  }))
  await expect(fs.stat(path.join(options.cacheDir, 'v11', 'metadata-cache'))).rejects.toMatchObject({ code: 'ENOENT' })
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

test.each([
  ['installed lockfile', path.join('node_modules', '.pnpm', 'lock.yaml'), 'packages: ['],
  ['modules manifest', path.join('node_modules', '.modules.yaml'), '{'],
])('prune fails before it removes anything when a registered project has a broken %s', async (_name, brokenFile, content) => {
  const options = createPruneOptions()
  const projectDir = path.join(options.cacheDir, '..', 'project')
  await fs.mkdir(path.join(projectDir, 'node_modules', '.pnpm'), { recursive: true })
  await fs.writeFile(path.join(projectDir, brokenFile), content)
  await registerProject(options.storeDir, projectDir)
  const unusedFile = path.join(options.storeDir, 'files', '00', 'unused')
  await fs.mkdir(path.dirname(unusedFile), { recursive: true })
  await fs.writeFile(unusedFile, 'unused')

  await expect(prune(options)).rejects.toMatchObject({ code: 'ERR_PNPM_STORE_DIR_PRUNE_IN_USE_FILES' })
  expect(await fs.readFile(unusedFile, 'utf8')).toBe('unused')
})

test('prune fails before it removes anything when the store index cannot be read', async () => {
  const options = createPruneOptions()
  const projectDir = path.join(options.cacheDir, '..', 'project')
  await fs.mkdir(path.join(projectDir, 'node_modules', '.pnpm'), { recursive: true })
  await fs.writeFile(path.join(projectDir, 'node_modules', '.pnpm', 'lock.yaml'), [
    "lockfileVersion: '9.0'",
    'importers:',
    "  '.': {}",
    'packages:',
    '  is-positive@1.0.0:',
    '    resolution: {integrity: sha512-AAAA}',
    'snapshots:',
    '  is-positive@1.0.0: {}',
    '',
  ].join('\n'))
  await registerProject(options.storeDir, projectDir)
  const unusedFile = path.join(options.storeDir, 'files', '00', 'unused')
  await fs.mkdir(path.dirname(unusedFile), { recursive: true })
  await fs.writeFile(unusedFile, 'unused')
  jest.spyOn(options.storeIndex, 'get').mockImplementation(() => {
    throw new Error('database disk image is malformed')
  })

  await expect(prune(options)).rejects.toMatchObject({ code: 'ERR_PNPM_STORE_DIR_PRUNE_IN_USE_FILES' })
  expect(await fs.readFile(unusedFile, 'utf8')).toBe('unused')
})

test('prune keeps a runtime whose index row is keyed with its manifest integrity', async () => {
  // pnpm 12 shares this store and keys a runtime row as
  // `package-manifest\t<manifest integrity>\t<integrity>\t<pkgId>`.
  const options = createPruneOptions()
  const projectDir = path.join(options.cacheDir, '..', 'project')
  await fs.mkdir(path.join(projectDir, 'node_modules', '.pnpm'), { recursive: true })
  const integrity = 'sha512-BBBB'
  await fs.writeFile(path.join(projectDir, 'node_modules', '.pnpm', 'lock.yaml'), JSON.stringify({
    lockfileVersion: '9.0',
    importers: { '.': { dependencies: { node: { specifier: 'runtime:22.0.0', version: 'runtime:22.0.0' } } } },
    packages: {
      'node@runtime:22.0.0': {
        resolution: {
          type: 'variations',
          variants: [{
            targets: [{ os: 'linux', cpu: 'x64' }],
            resolution: { type: 'binary', archive: 'tarball', url: 'https://example.com/node.tgz', integrity, bin: { node: 'bin/node' } },
          }],
        },
        version: '22.0.0',
      },
    },
    snapshots: { 'node@runtime:22.0.0': {} },
  }))
  await registerProject(options.storeDir, projectDir)
  const files = new Map([
    ['package.json', { digest: 'aa'.repeat(32), mode: 0o644, size: 2 }],
    ['bin/node', { digest: 'bb'.repeat(32), mode: 0o755, size: 2 }],
  ])
  const blobPaths = Array.from(files.values(), ({ digest, mode }) => getFilePathByModeInCafs(options.storeDir, digest, mode))
  await Promise.all(blobPaths.map(async (blobPath) => {
    await fs.mkdir(path.dirname(blobPath), { recursive: true })
    await fs.writeFile(blobPath, '{}')
  }))
  const key = `package-manifest\tsha512-CCCC\t${integrity}\tnode@runtime:22.0.0`
  options.storeIndex.set(key, { algo: 'sha512', files })

  await prune(options)

  expect(options.storeIndex.has(key)).toBe(true)
  expect(blobPaths.filter((blobPath) => !existsSync(blobPath))).toStrictEqual([])
})

function createPruneOptions () {
  const directory = temporaryDirectory()
  temporaryDirectories.push(directory)
  const storeDir = path.join(directory, 'store')
  const storeIndex = new StoreIndex(storeDir)
  storeIndexes.push(storeIndex)
  return { storeDir, cacheDir: path.join(directory, 'cache'), storeIndex }
}
