/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import { createDirectoryFetcher } from '@pnpm/directory-fetcher'
// @ts-expect-error
import { debug } from '@pnpm/logger'
import { fixtures } from '@pnpm/test-fixtures'

const f = fixtures(__dirname)
jest.mock('@pnpm/logger', () => {
  const debug = jest.fn()
  return ({ debug, logger: () => ({ debug }) })
})

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
  expect(fetchResult.filesIndex['package.json']).toBe(path.resolve('package.json'))

  // Only those files are included which would get published
  expect(Object.keys(fetchResult.filesIndex).sort()).toStrictEqual([
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
  expect(fetchResult.filesIndex['package.json']).toBe(path.resolve('package.json'))

  // Only those files are included which would get published
  expect(Object.keys(fetchResult.filesIndex).sort()).toStrictEqual([
    'index.js',
    'package.json',
    'test.js',
  ])
})

test('fetch a directory that has no package.json', async () => {
  process.chdir(f.find('no-manifest'))
  const fetcher = createDirectoryFetcher()
  const manifest = {
    resolve: jest.fn(),
    reject: jest.fn(),
  }

  // eslint-disable-next-line
  const fetchResult = await fetcher.directory({} as any, {
    directory: '.',
    type: 'directory',
  }, {
    lockfileDir: process.cwd(),
    manifest,
  })

  expect(manifest.resolve).toBeCalledWith({})
  expect(fetchResult.local).toBe(true)
  expect(fetchResult.packageImportMethod).toBe('hardlink')
  expect(fetchResult.filesIndex['index.js']).toBe(path.resolve('index.js'))

  // Only those files are included which would get published
  expect(Object.keys(fetchResult.filesIndex).sort()).toStrictEqual([
    'index.js',
  ])
})

test('fetch does not fail on package with broken symlink', async () => {
  debug.mockClear()
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
  expect(fetchResult.filesIndex['package.json']).toBe(path.resolve('package.json'))

  // Only those files are included which would get published
  expect(Object.keys(fetchResult.filesIndex).sort()).toStrictEqual([
    'index.js',
    'package.json',
  ])
  expect(debug).toHaveBeenCalledWith({ brokenSymlink: path.resolve('not-exists') })
})
