import fs from 'fs'
import path from 'path'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'
import { cleanExpiredDlxCache, cleanOrphans } from './cleanExpiredDlxCache'

const createCacheKey = (...pkgs: string[]): string => dlx.createCacheKey(pkgs, { default: 'https://registry.npmjs.com/' })

function createSampleDlxCacheLinkTarget (dirPath: string): void {
  fs.mkdirSync(path.join(dirPath, 'node_modules', '.pnpm'), { recursive: true })
  fs.mkdirSync(path.join(dirPath, 'node_modules', '.bin'), { recursive: true })
  fs.writeFileSync(path.join(dirPath, 'node_modules', '.modules.yaml'), '')
  fs.writeFileSync(path.join(dirPath, 'package.json'), '')
  fs.writeFileSync(path.join(dirPath, 'pnpm-lock.yaml'), '')
}

function createSampleDlxCacheItem (cacheDir: string, cmd: string, now: Date, age: number): void {
  const hash = createCacheKey(cmd)
  const newDate = new Date(now.getTime() - age * 60_000)
  const timeError = 432 // just an arbitrary amount, nothing is special about this number
  const pid = 71014 // just an arbitrary number to represent pid
  const targetName = `${(newDate.getTime() - timeError).toString(16)}-${pid.toString(16)}`
  const linkTarget = path.join(cacheDir, 'dlx', hash, targetName)
  const linkPath = path.join(cacheDir, 'dlx', hash, 'pkg')
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

  const readdirSyncSpy = jest.spyOn(fs, 'readdirSync')
  const lstatSpy = jest.spyOn(fs.promises, 'lstat')
  const rmSpy = jest.spyOn(fs.promises, 'rm')

  await cleanExpiredDlxCache({
    cacheDir,
    dlxCacheMaxAge,
    now,
  })

  expect(fs.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('foo'))).length).toBe(2)
  expect(fs.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('bar'))).length).toBe(2)
  expect(fs.existsSync(path.join(cacheDir, 'dlx', createCacheKey('baz')))).toBeFalsy()

  expect(readdirSyncSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx'), expect.anything())
  for (const key of ['foo', 'bar', 'baz']) {
    expect(lstatSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx', createCacheKey(key), 'pkg'))
  }
  expect(rmSpy).not.toHaveBeenCalledWith(
    expect.stringContaining(path.join(cacheDir, 'dlx', createCacheKey('foo'))),
    expect.anything()
  )
  expect(rmSpy).not.toHaveBeenCalledWith(
    expect.stringContaining(path.join(cacheDir, 'dlx', createCacheKey('bar'))),
    expect.anything()
  )
  expect(rmSpy).toHaveBeenCalledWith(
    expect.stringContaining(path.join(cacheDir, 'dlx', createCacheKey('baz'))),
    { recursive: true, force: true }
  )

  readdirSyncSpy.mockRestore()
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

  const readdirSyncSpy = jest.spyOn(fs, 'readdirSync')
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

  expect(readdirSyncSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx'), expect.anything())
  expect(lstatSpy).not.toHaveBeenCalled()
  for (const key of ['foo', 'bar', 'baz']) {
    expect(rmSpy).toHaveBeenCalledWith(path.join(cacheDir, 'dlx', createCacheKey(key)), { recursive: true, force: true })
  }

  readdirSyncSpy.mockRestore()
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

  const dlxCacheDir = path.join(cacheDir, 'dlx')

  const entries = fs.readdirSync(dlxCacheDir).sort()
  expect(entries).toStrictEqual(
    ['foo', 'bar', 'baz']
      .map(cmd => createCacheKey(cmd))
      .sort()
  )

  for (const entry of entries) {
    expect(fs.readdirSync(path.join(dlxCacheDir, entry)).length).toBe(2)
  }

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
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createCacheKey('foo'), `${now.getTime().toString(16)}-${(7000).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createCacheKey('foo'), `${now.getTime().toString(16)}-${(7005).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createCacheKey('foo'), `${now.getTime().toString(16)}-${(7102).toString(16)}`))
  expect(fs.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('foo'))).length).toBe(5)

  // has no link, only orphans
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createCacheKey('bar'), `${now.getTime().toString(16)}-${(7000).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createCacheKey('bar'), `${now.getTime().toString(16)}-${(7005).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createCacheKey('bar'), `${now.getTime().toString(16)}-${(7102).toString(16)}`))
  expect(fs.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('bar'))).length).toBe(3)

  await cleanOrphans(path.join(cacheDir, 'dlx'))

  // expecting all subdirectories that aren't pointed to by `link` to be deleted.
  expect(fs.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('foo'))).length).toBe(2)

  // expecting directory that doesn't contain `link` to be deleted.
  expect(fs.existsSync(path.join(cacheDir, 'dlx', createCacheKey('bar')))).toBe(false)
})
