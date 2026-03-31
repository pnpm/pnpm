/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import path from 'path'
import { createDirectoryFetcher } from '../src'
// @ts-expect-error
import { debug } from '@pnpm/logger'
import { fixtures } from '@pnpm/test-fixtures'

function rimrafSync (target: string) {
  fs.rmSync(target, { recursive: true, force: true })
}

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

  // eslint-disable-next-line
  const fetchResult = await fetcher.directory({} as any, {
    directory: '.',
    type: 'directory',
  }, {
    lockfileDir: process.cwd(),
    readManifest: true,
  })

  expect(fetchResult.manifest).toEqual(undefined)
  expect(fetchResult.local).toBe(true)
  expect(fetchResult.packageImportMethod).toBe('hardlink')
  expect(fetchResult.filesIndex['index.js']).toBe(path.resolve('index.js'))

  // Only those files are included which would get published
  expect(Object.keys(fetchResult.filesIndex).sort()).toStrictEqual([
    'index.js',
  ])
})

describe('package with broken symlink', () => {
  const brokenSymlinkPkg = f.find('pkg-with-broken-symlink')
  beforeAll(() => {
    // Git on Windows often checks out symlinks as plain files; create a real broken symlink at runtime (same as other symlink tests in this file).
    process.chdir(brokenSymlinkPkg)
    rimrafSync('not-exists')
    fs.symlinkSync('this-symlink-target-does-not-exist', path.resolve('not-exists'), 'file')
  })

  test('fetch does not fail on package with broken symlink', async () => {
    debug.mockClear()
    process.chdir(brokenSymlinkPkg)
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
    expect(fetchResult.filesIndex['package.json']).toBe(path.resolve('package.json'))
    expect(fetchResult.filesIndex['index.js']).toBe(indexJsPath)
    expect(fetchResult.filesIndex['src/index.js']).toBe(path.join(srcPath, 'index.js'))
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
    expect(fetchResult.filesIndex['package.json']).toBe(path.resolve('package.json'))
    expect(fetchResult.filesIndex['index.js']).toBe(path.resolve('index.js'))
    expect(fetchResult.filesIndex['src/index.js']).toBe(path.resolve('src/index.js'))
  })
})
