/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { jest } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { rimrafSync } from '@zkochan/rimraf'

const debug = jest.fn()
jest.unstable_mockModule('@pnpm/logger', () => {
  return ({ globalWarn: jest.fn(), debug, logger: () => ({ debug }) })
})
const { createDirectoryFetcher } = await import('@pnpm/fetching.directory-fetcher')

const f = fixtures(import.meta.dirname)

test('fetch including only package files', async () => {
  process.chdir(f.find('simple-pkg'))
  const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: true })

  // eslint-disable-next-line
  const fetchResult = await fetcher.directory({} as any, {
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

test('fetch including all files', async () => {
  process.chdir(f.find('simple-pkg'))
  const fetcher = createDirectoryFetcher()

  // eslint-disable-next-line
  const fetchResult = await fetcher.directory({} as any, {
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

test('fetch a directory that has no package.json', async () => {
  process.chdir(f.find('no-manifest'))
  const fetcher = createDirectoryFetcher()

  // eslint-disable-next-line
  const fetchResult = await fetcher.directory({} as any, {
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
  process.chdir(f.find('pkg-with-broken-symlink'))
  const fetcher = createDirectoryFetcher()

  // eslint-disable-next-line
  const fetchResult = await fetcher.directory({} as any, {
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
  const absDir = f.find('simple-pkg')
  const fetcher = createDirectoryFetcher({ includeOnlyPackageFiles: true })

  // lockfileDir is unrelated to the directory being fetched. When the
  // stored directory is absolute (e.g. cross-drive `file:` deps on Windows)
  // the fetcher must use the absolute path as-is rather than joining it
  // onto lockfileDir.
  // eslint-disable-next-line
  const fetchResult = await fetcher.directory({} as any, {
    directory: absDir,
    type: 'directory',
  }, {
    lockfileDir: f.find('no-manifest'),
  })

  expect(fetchResult.local).toBe(true)
  expect(fetchResult.filesMap.get('package.json')).toBe(path.join(absDir, 'package.json'))
})

describe('fetch resolves symlinked files to their real locations', () => {
  const indexJsPath = path.join(f.find('no-manifest'), 'index.js')
  const srcPath = f.find('simple-pkg')
  beforeAll(async () => {
    process.chdir(f.find('pkg-with-symlinked-dir-and-files'))
    rimrafSync('index.js')
    fs.symlinkSync(indexJsPath, path.resolve('index.js'), 'file')
    rimrafSync('src')
    fs.symlinkSync(srcPath, path.resolve('src'), 'dir')
  })
  test('fetch resolves symlinked files to their real locations', async () => {
    const fetcher = createDirectoryFetcher({ resolveSymlinks: true })
    // eslint-disable-next-line
    const fetchResult = await fetcher.directory({} as any, {
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

    // eslint-disable-next-line
    const fetchResult = await fetcher.directory({} as any, {
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
