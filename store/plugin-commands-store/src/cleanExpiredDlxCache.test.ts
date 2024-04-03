import fs from 'fs'
import path from 'path'
import util from 'util'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { prepareEmpty } from '@pnpm/prepare'
import { cleanExpiredDlxCache, cleanOrphans } from './cleanExpiredDlxCache'

function readDlxCachePath (cachePath: string): string[] | 'ENOENT' {
  let names: string[]
  try {
    names = fs.readdirSync(cachePath, 'utf-8')
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return 'ENOENT'
    }
    throw err
  }
  return names
    .map(sanitizeDlxCacheComponent)
    .sort()
}

function sanitizeDlxCacheComponent (cacheName: string): string {
  if (cacheName === 'link') return cacheName
  const segments = cacheName.split('-')
  if (segments.length !== 2) {
    throw new Error(`Unexpected name: ${cacheName}`)
  }
  const [date, pid] = segments
  if (!/[0-9a-f]+/.test(date) && !/[0-9a-f]+/.test(pid)) {
    throw new Error(`Name ${cacheName} doesn't end with 2 hex numbers`)
  }
  return '***********-*****'
}

function createSampleDlxCacheLinkTarget (dirPath: string): void {
  fs.mkdirSync(path.join(dirPath, 'node_modules', '.pnpm'), { recursive: true })
  fs.mkdirSync(path.join(dirPath, 'node_modules', '.bin'), { recursive: true })
  fs.writeFileSync(path.join(dirPath, 'node_modules', '.modules.yaml'), '')
  fs.writeFileSync(path.join(dirPath, 'package.json'), '')
  fs.writeFileSync(path.join(dirPath, 'pnpm-lock.yaml'), '')
}

function createSampleDlxCacheItem (cacheDir: string, cmd: string, now: Date, age: number): void {
  const hash = createBase32Hash(cmd)
  const newDate = new Date(now.getTime() - age * 60_000)
  const timeError = 432 // just an arbitrary amount, nothing is special about this number
  const pid = 71014 // just an arbitrary number to represent pid
  const targetName = `${(newDate.getTime() - timeError).toString(16)}-${pid.toString(16)}`
  const linkTarget = path.join(cacheDir, 'dlx', hash, targetName)
  const linkPath = path.join(cacheDir, 'dlx', hash, 'link')
  createSampleDlxCacheLinkTarget(linkTarget)
  fs.symlinkSync(linkTarget, linkPath, 'junction')
  fs.lutimesSync(linkPath, newDate, newDate)
}

function createSampleDlxCacheFsTree (cacheDir: string, now: Date, ageTable: Record<string, number>): void {
  for (const [cmd, age] of Object.entries(ageTable)) {
    createSampleDlxCacheItem(cacheDir, cmd, now, age)
  }
}

afterEach(() => {
  jest.restoreAllMocks()
})

test('cleanExpiredCache removes items that outlive dlxCacheMaxAge', async () => {
  prepareEmpty()

  const cacheDir = path.resolve('cache')
  const dlxCacheMaxAge = 7
  const now = new Date()

  createSampleDlxCacheFsTree(cacheDir, now, {
    foo: 1,
    bar: 5,
    baz: 20,
  })

  const readdirSpy = jest.spyOn(fs.promises, 'readdir')
  const lstatSpy = jest.spyOn(fs.promises, 'lstat')
  const rmSpy = jest.spyOn(fs.promises, 'rm')

  await cleanExpiredDlxCache({
    cacheDir,
    dlxCacheMaxAge,
    now,
  })

  expect({
    foo: readDlxCachePath(path.join(cacheDir, 'dlx', createBase32Hash('foo'))),
    bar: readDlxCachePath(path.join(cacheDir, 'dlx', createBase32Hash('bar'))),
    baz: readDlxCachePath(path.join(cacheDir, 'dlx', createBase32Hash('baz'))),
  }).toStrictEqual({
    foo: ['link', '***********-*****'].sort(),
    bar: ['link', '***********-*****'].sort(),
    baz: 'ENOENT',
  })

  expect(readdirSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx'), expect.anything())
  for (const key of ['foo', 'bar', 'baz']) {
    expect(lstatSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx', createBase32Hash(key), 'link'))
  }
  expect(rmSpy).not.toHaveBeenCalledWith(
    expect.stringContaining(path.join(cacheDir, 'dlx', createBase32Hash('foo'))),
    expect.anything()
  )
  expect(rmSpy).not.toHaveBeenCalledWith(
    expect.stringContaining(path.join(cacheDir, 'dlx', createBase32Hash('bar'))),
    expect.anything()
  )
  expect(rmSpy).toHaveBeenCalledWith(
    expect.stringContaining(path.join(cacheDir, 'dlx', createBase32Hash('baz'))),
    { recursive: true }
  )

  readdirSpy.mockRestore()
  lstatSpy.mockRestore()
  rmSpy.mockRestore()
})

test('cleanExpiredCache removes all directories without checking stat if dlxCacheMaxAge is 0', async () => {
  prepareEmpty()

  const cacheDir = path.resolve('cache')
  const dlxCacheMaxAge = 0
  const now = new Date()

  createSampleDlxCacheFsTree(cacheDir, now, {
    foo: 1,
    bar: 5,
    baz: 20,
  })

  const readdirSpy = jest.spyOn(fs.promises, 'readdir')
  const lstatSpy = jest.spyOn(fs.promises, 'lstat')
  const rmSpy = jest.spyOn(fs.promises, 'rm')

  await cleanExpiredDlxCache({
    cacheDir,
    dlxCacheMaxAge,
    now,
  })

  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx'))
      .sort()
  ).toStrictEqual([])

  expect(readdirSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx'), expect.anything())
  expect(lstatSpy).not.toHaveBeenCalled()
  for (const key of ['foo', 'bar', 'baz']) {
    expect(rmSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx', createBase32Hash(key)), { recursive: true })
  }

  readdirSpy.mockRestore()
  lstatSpy.mockRestore()
  rmSpy.mockRestore()
})

test('cleanExpiredCache does nothing if dlxCacheMaxAge is Infinity', async () => {
  prepareEmpty()

  const cacheDir = path.resolve('cache')
  const dlxCacheMaxAge = Infinity
  const now = new Date()

  createSampleDlxCacheFsTree(cacheDir, now, {
    foo: 1,
    bar: 5,
    baz: 20,
  })

  const readdirSpy = jest.spyOn(fs.promises, 'readdir')
  const lstatSpy = jest.spyOn(fs.promises, 'lstat')
  const rmSpy = jest.spyOn(fs.promises, 'rm')

  await cleanExpiredDlxCache({
    cacheDir,
    dlxCacheMaxAge,
    now,
  })

  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx'))
      .sort()
  ).toStrictEqual(
    ['foo', 'bar', 'baz']
      .map(createBase32Hash)
      .sort()
  )

  expect(
    Object.fromEntries(
      fs.readdirSync(path.join(cacheDir, 'dlx'))
        .sort()
        .map(dlxCacheName => [
          dlxCacheName,
          fs.readdirSync(path.join(cacheDir, 'dlx', dlxCacheName))
            .map(sanitizeDlxCacheComponent)
            .sort(),
        ])
    )
  ).toStrictEqual({
    [createBase32Hash('foo')]: ['link', '***********-*****'].sort(),
    [createBase32Hash('bar')]: ['link', '***********-*****'].sort(),
    [createBase32Hash('baz')]: ['link', '***********-*****'].sort(),
  })

  expect(readdirSpy).not.toHaveBeenCalled()
  expect(lstatSpy).not.toHaveBeenCalled()
  expect(rmSpy).not.toHaveBeenCalled()

  readdirSpy.mockRestore()
  lstatSpy.mockRestore()
  rmSpy.mockRestore()
})

test("cleanOrphans deletes dirs that don't contain `link` and subdirs that aren't pointed to by `link` from the same parent", async () => {
  prepareEmpty()

  const cacheDir = path.resolve('cache')
  const now = new Date()

  // has link and orphans
  createSampleDlxCacheItem(cacheDir, 'foo', now, 0)
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createBase32Hash('foo'), `${now.getTime().toString(16)}-${(7000).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createBase32Hash('foo'), `${now.getTime().toString(16)}-${(7005).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createBase32Hash('foo'), `${now.getTime().toString(16)}-${(7102).toString(16)}`))
  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx', createBase32Hash('foo')))
      .map(sanitizeDlxCacheComponent)
      .sort()
  ).toStrictEqual([
    'link',
    '***********-*****',
    '***********-*****',
    '***********-*****',
    '***********-*****',
  ].sort())

  // has no link, only orphans
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createBase32Hash('bar'), `${now.getTime().toString(16)}-${(7000).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createBase32Hash('bar'), `${now.getTime().toString(16)}-${(7005).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createBase32Hash('bar'), `${now.getTime().toString(16)}-${(7102).toString(16)}`))
  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx', createBase32Hash('bar')))
      .map(sanitizeDlxCacheComponent)
      .sort()
  ).toStrictEqual([
    '***********-*****',
    '***********-*****',
    '***********-*****',
  ].sort())

  await cleanOrphans(path.join(cacheDir, 'dlx'))

  // expecting all subdirectories that aren't pointed to by `link` to be deleted.
  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx', createBase32Hash('foo')))
      .map(sanitizeDlxCacheComponent)
      .sort()
  ).toStrictEqual([
    'link',
    '***********-*****',
  ].sort())

  // expecting directory that doesn't contain `link` to be deleted.
  expect(fs.existsSync(path.join(cacheDir, 'dlx', createBase32Hash('bar')))).toBe(false)
})
