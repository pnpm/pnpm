/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { beforeAll, describe, expect, jest, test } from '@jest/globals'
import type { Cafs } from '@pnpm/store.cafs-types'
import { fixtures } from '@pnpm/test-fixtures'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import { rimrafSync } from '@zkochan/rimraf'

const debug = jest.fn()
jest.unstable_mockModule('@pnpm/logger', () => {
  return ({ globalWarn: jest.fn(), debug, logger: () => ({ debug }) })
})
const { createDirectoryFetcher } = await import('@pnpm/fetching.directory-fetcher')

const testFixtures = fixtures(import.meta.dirname)
const unusedCafs = {} as Cafs

test('fetch including only package files', async () => {
  process.chdir(testFixtures.find('simple-pkg'))
  const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: true })

  const fetchResult = await fetcher.directory(unusedCafs, {
    directory: '.',
    type: 'directory',
  }, {
    lockfileDir: process.cwd(),
  })

  expect(fetchResult.local).toBe(true)
  expect(fetchResult.packageImportMethod).toBe('hardlink')
  expect(fetchResult.filesMap.get('package.json')).toBe(path.resolve('package.json'))

  // Only those files are included which would get published
  expect(Array.from(fetchResult.filesMap.keys()).sort(lexCompare)).toStrictEqual([
    'index.js',
    'package.json',
  ])
})

test('fetch including only package files of a package with package.yaml', async () => {
  const packageDir = testFixtures.find('pkg-with-package-yaml')
  const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: true })

  const fetchResult = await fetcher.directory(unusedCafs, {
    directory: '.',
    type: 'directory',
  }, {
    lockfileDir: packageDir,
  })

  expect(Array.from(fetchResult.filesMap.keys()).sort(lexCompare)).toStrictEqual([
    'out/index.js',
    'package.yaml',
  ])
})

test('fetch package files includes bundled dependencies under a listed directory', async () => {
  const packageDir = testFixtures.find('standalone-pkg')
  const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: true })

  const fetchResult = await fetcher.directory(unusedCafs, {
    directory: '.',
    type: 'directory',
  }, {
    lockfileDir: packageDir,
  })

  expect(Array.from(fetchResult.filesMap.keys()).sort(lexCompare)).toStrictEqual([
    'dist/node_modules/node-gyp/bin/node-gyp.js',
    'package.json',
    'pnpm',
  ])
})

test('fetch including all files', async () => {
  process.chdir(testFixtures.find('simple-pkg'))
  const fetcher = createDirectoryFetcher()

  const fetchResult = await fetcher.directory(unusedCafs, {
    directory: '.',
    type: 'directory',
  }, {
    lockfileDir: process.cwd(),
  })

  expect(fetchResult.local).toBe(true)
  expect(fetchResult.packageImportMethod).toBe('hardlink')
  expect(fetchResult.filesMap.get('package.json')).toBe(path.resolve('package.json'))

  // Only those files are included which would get published
  expect(Array.from(fetchResult.filesMap.keys()).sort(lexCompare)).toStrictEqual([
    'index.js',
    'package.json',
    'test.js',
  ])
})

test('fetch can override the local directory package import method', async () => {
  process.chdir(testFixtures.find('simple-pkg'))
  const fetcher = createDirectoryFetcher({ localDirPackageImportMethod: 'clone-or-copy' })

  const fetchResult = await fetcher.directory(unusedCafs, {
    directory: '.',
    type: 'directory',
  }, {
    lockfileDir: process.cwd(),
  })

  expect(fetchResult.packageImportMethod).toBe('clone-or-copy')
})

test('fetch a directory that has no package.json', async () => {
  process.chdir(testFixtures.find('no-manifest'))
  const fetcher = createDirectoryFetcher()

  const fetchResult = await fetcher.directory(unusedCafs, {
    directory: '.',
    type: 'directory',
  }, {
    lockfileDir: process.cwd(),
    readManifest: true,
  })

  expect(fetchResult.manifest).toBeUndefined()
  expect(fetchResult.local).toBe(true)
  expect(fetchResult.packageImportMethod).toBe('hardlink')
  expect(fetchResult.filesMap.get('index.js')).toBe(path.resolve('index.js'))

  // Only those files are included which would get published
  expect(Array.from(fetchResult.filesMap.keys()).sort(lexCompare)).toStrictEqual([
    'index.js',
  ])
})

test('fetch does not fail on package with broken symlink', async () => {
  jest.mocked(debug).mockClear()
  const dir = testFixtures.prepare('pkg-with-broken-symlink')
  fs.symlinkSync('broken-symlink', path.join(dir, 'not-exists'))
  process.chdir(dir)
  const fetcher = createDirectoryFetcher()

  const fetchResult = await fetcher.directory(unusedCafs, {
    directory: '.',
    type: 'directory',
  }, {
    lockfileDir: process.cwd(),
  })

  expect(fetchResult.local).toBe(true)
  expect(fetchResult.packageImportMethod).toBe('hardlink')
  expect(fetchResult.filesMap.get('package.json')).toBe(path.resolve('package.json'))

  // Only those files are included which would get published
  expect(Array.from(fetchResult.filesMap.keys()).sort(lexCompare)).toStrictEqual([
    'index.js',
    'package.json',
  ])
  expect(debug).toHaveBeenCalledWith({ brokenSymlink: path.resolve('not-exists') })
})

test('fetch respects absolute directory regardless of lockfileDir', async () => {
  const absDir = testFixtures.find('simple-pkg')
  const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: true })

  // lockfileDir is unrelated to the directory being fetched. When the
  // stored directory is absolute (e.g. cross-drive `file:` deps on Windows)
  // the fetcher must use the absolute path as-is rather than joining it
  // onto lockfileDir.
  const fetchResult = await fetcher.directory(unusedCafs, {
    directory: absDir,
    type: 'directory',
  }, {
    lockfileDir: testFixtures.find('no-manifest'),
  })

  expect(fetchResult.local).toBe(true)
  expect(fetchResult.filesMap.get('package.json')).toBe(path.join(absDir, 'package.json'))
})

test('fetch tolerates a publish directory that has not been built yet', async () => {
  const projectDir = fs.mkdtempSync(path.join(os.tmpdir(), 'directory-fetcher-'))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({ name: 'x', version: '1.0.0', publishConfig: { directory: 'dist' } }))
  const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: true })

  const fetchResult = await fetcher.directory(unusedCafs, {
    directory: 'dist',
    type: 'directory',
  }, {
    lockfileDir: projectDir,
  })

  expect(fetchResult.sourceExists).toBe(false)
  expect(fetchResult.filesMap.size).toBe(0)
  expect(fetchResult.manifest?.name).toBe('x')
})

test('fetch fails for a missing directory that no project publishes from', async () => {
  const projectDir = fs.mkdtempSync(path.join(os.tmpdir(), 'directory-fetcher-'))
  fs.writeFileSync(path.join(projectDir, 'package.json'), JSON.stringify({ name: 'x', version: '1.0.0', publishConfig: { directory: 'dist' } }))
  const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: true })

  await expect(fetcher.directory(unusedCafs, {
    directory: 'missing',
    type: 'directory',
  }, {
    lockfileDir: projectDir,
  })).rejects.toThrow()
})

describe('fetch resolves symlinked files to their real locations', () => {
  const indexJsPath = path.join(testFixtures.find('no-manifest'), 'index.js')
  const srcPath = testFixtures.find('simple-pkg')
  beforeAll(async () => {
    process.chdir(testFixtures.find('pkg-with-symlinked-dir-and-files'))
    rimrafSync('index.js')
    fs.symlinkSync(indexJsPath, path.resolve('index.js'), 'file')
    rimrafSync('src')
    fs.symlinkSync(srcPath, path.resolve('src'), 'dir')
  })
  test('fetch resolves symlinked files to their real locations', async () => {
    const fetcher = createDirectoryFetcher({ resolveSymlinks: true })
    const fetchResult = await fetcher.directory(unusedCafs, {
      directory: '.',
      type: 'directory',
    }, {
      lockfileDir: process.cwd(),
    })

    expect(fetchResult.local).toBe(true)
    expect(fetchResult.packageImportMethod).toBe('hardlink')
    expect(fetchResult.filesMap.get('package.json')).toBe(path.resolve('package.json'))
    expect(fetchResult.filesMap.get('index.js')).toBe(indexJsPath)
    expect(fetchResult.filesMap.get('src/index.js')).toBe(path.join(srcPath, 'index.js'))
  })
  test('fetch does not resolve symlinked files to their real locations by default', async () => {
    const fetcher = createDirectoryFetcher()

    const fetchResult = await fetcher.directory(unusedCafs, {
      directory: '.',
      type: 'directory',
    }, {
      lockfileDir: process.cwd(),
    })

    expect(fetchResult.local).toBe(true)
    expect(fetchResult.packageImportMethod).toBe('hardlink')
    expect(fetchResult.filesMap.get('package.json')).toBe(path.resolve('package.json'))
    expect(fetchResult.filesMap.get('index.js')).toBe(path.resolve('index.js'))
    expect(fetchResult.filesMap.get('src/index.js')).toBe(path.resolve('src/index.js'))
  })
})


test.each(['file', 'directory'])('all-files deployment rejects an external %s symlink', async (kind) => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'directory-fetcher-'))
  try {
    const pkgDir = path.join(tmp, 'package')
    const outside = path.join(tmp, 'outside')
    fs.mkdirSync(pkgDir)
    fs.mkdirSync(outside)
    fs.writeFileSync(path.join(pkgDir, 'package.json'), '{"name":"test","version":"1.0.0"}')
    fs.writeFileSync(path.join(outside, 'secret'), 'private')
    const target = kind === 'file' ? path.join(outside, 'secret') : outside
    fs.symlinkSync(target, path.join(pkgDir, 'linked'), kind === 'file' ? 'file' : 'junction')
    const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: false })

    await expect(fetcher.directory(unusedCafs, { directory: pkgDir, type: 'directory' }, { lockfileDir: tmp }))
      .rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_PATH' })
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true })
  }
})

test('all-files deployment follows internal symlinks under a symlinked package root', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'directory-fetcher-'))
  try {
    const pkgDir = path.join(tmp, 'package')
    const linkedRoot = path.join(tmp, 'linked-root')
    fs.mkdirSync(pkgDir)
    fs.writeFileSync(path.join(pkgDir, 'package.json'), '{"name":"test","version":"1.0.0"}')
    fs.writeFileSync(path.join(pkgDir, 'source'), 'public')
    fs.symlinkSync(path.join(pkgDir, 'source'), path.join(pkgDir, 'linked'), 'file')
    fs.symlinkSync(pkgDir, linkedRoot, 'junction')
    const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: false })
    const result = await fetcher.directory(unusedCafs, { directory: linkedRoot, type: 'directory' }, { lockfileDir: tmp })

    expect(result.filesMap.get('linked')).toBe(path.join(linkedRoot, 'linked'))
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true })
  }
})
