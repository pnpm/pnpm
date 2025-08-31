import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { jest } from '@jest/globals'

const fsOriginal = await import('fs')
jest.unstable_mockModule('fs', () => ({
  ...fsOriginal,
  readdirSync: jest.fn(fsOriginal.readdirSync),
  promises: {
    ...fsOriginal.promises,
    readdir: jest.fn(fsOriginal.promises.readdir),
    lstat: jest.fn(fsOriginal.promises.lstat),
    rm: jest.fn(fsOriginal.promises.rm),
  },
}))
const fs = await import('fs')
const { cleanExpiredDlxCache, cleanOrphans } = await import('./cleanExpiredDlxCache.js')
const { dlx } = await import('@pnpm/plugin-commands-script-runners')

beforeEach(() => {
  jest.mocked(fs.readdirSync).mockClear()
  jest.mocked(fs.promises.readdir).mockClear()
  jest.mocked(fs.promises.lstat).mockClear()
  jest.mocked(fs.promises.rm).mockClear()
})

const createCacheKey = (...packages: string[]): string => dlx.createCacheKey({
  packages,
  registries: { default: 'https://registry.npmjs.com/' },
})

function createSampleDlxCacheLinkTarget (dirPath: string): void {
  fsOriginal.mkdirSync(path.join(dirPath, 'node_modules', '.pnpm'), { recursive: true })
  fsOriginal.mkdirSync(path.join(dirPath, 'node_modules', '.bin'), { recursive: true })
  fsOriginal.writeFileSync(path.join(dirPath, 'node_modules', '.modules.yaml'), '')
  fsOriginal.writeFileSync(path.join(dirPath, 'package.json'), '')
  fsOriginal.writeFileSync(path.join(dirPath, 'pnpm-lock.yaml'), '')
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
  fsOriginal.symlinkSync(linkTarget, linkPath, 'junction')
  fsOriginal.lutimesSync(linkPath, newDate, newDate)
}

function createSampleDlxCacheFsTree (cacheDir: string, now: Date, ageTable: Record<string, number>): void {
  for (const [cmd, age] of Object.entries(ageTable)) {
    createSampleDlxCacheItem(cacheDir, cmd, now, age)
  }
}

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

  await cleanExpiredDlxCache({
    cacheDir,
    dlxCacheMaxAge,
    now,
  })

  expect(fsOriginal.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('foo')))).toHaveLength(2)
  expect(fsOriginal.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('bar')))).toHaveLength(2)
  expect(fsOriginal.existsSync(path.join(cacheDir, 'dlx', createCacheKey('baz')))).toBeFalsy()

  expect(fs.readdirSync).toHaveBeenCalledWith(path.join(cacheDir, 'dlx'), expect.anything())
  for (const key of ['foo', 'bar', 'baz']) {
    expect(fs.promises.lstat).toHaveBeenCalledWith(path.join(cacheDir, 'dlx', createCacheKey(key), 'pkg'))
  }
  expect(fs.promises.rm).not.toHaveBeenCalledWith(
    expect.stringContaining(path.join(cacheDir, 'dlx', createCacheKey('foo'))),
    expect.anything()
  )
  expect(fs.promises.rm).not.toHaveBeenCalledWith(
    expect.stringContaining(path.join(cacheDir, 'dlx', createCacheKey('bar'))),
    expect.anything()
  )
  expect(fs.promises.rm).toHaveBeenCalledWith(
    expect.stringContaining(path.join(cacheDir, 'dlx', createCacheKey('baz'))),
    { recursive: true, force: true }
  )
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

  await cleanExpiredDlxCache({
    cacheDir,
    dlxCacheMaxAge,
    now,
  })

  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx'))
      .sort()
  ).toStrictEqual([])

  expect(fs.readdirSync).toHaveBeenCalledWith(path.join(cacheDir, 'dlx'), expect.anything())
  expect(fs.promises.lstat).not.toHaveBeenCalled()
  for (const key of ['foo', 'bar', 'baz']) {
    expect(fs.promises.rm).toHaveBeenCalledWith(path.join(cacheDir, 'dlx', createCacheKey(key)), { recursive: true, force: true })
  }
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
    expect(fs.readdirSync(path.join(dlxCacheDir, entry))).toHaveLength(2)
  }

  expect(fs.promises.readdir).not.toHaveBeenCalled()
  expect(fs.promises.lstat).not.toHaveBeenCalled()
  expect(fs.promises.rm).not.toHaveBeenCalled()
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
  expect(fsOriginal.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('foo')))).toHaveLength(5)

  // has no link, only orphans
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createCacheKey('bar'), `${now.getTime().toString(16)}-${(7000).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createCacheKey('bar'), `${now.getTime().toString(16)}-${(7005).toString(16)}`))
  createSampleDlxCacheLinkTarget(path.join(cacheDir, 'dlx', createCacheKey('bar'), `${now.getTime().toString(16)}-${(7102).toString(16)}`))
  expect(fs.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('bar')))).toHaveLength(3)

  await cleanOrphans(path.join(cacheDir, 'dlx'))

  // expecting all subdirectories that aren't pointed to by `link` to be deleted.
  expect(fs.readdirSync(path.join(cacheDir, 'dlx', createCacheKey('foo')))).toHaveLength(2)

  // expecting directory that doesn't contain `link` to be deleted.
  expect(fs.existsSync(path.join(cacheDir, 'dlx', createCacheKey('bar')))).toBe(false)
})
