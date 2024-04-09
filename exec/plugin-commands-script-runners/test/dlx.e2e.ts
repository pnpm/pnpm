import fs from 'fs'
import path from 'path'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { add } from '@pnpm/plugin-commands-installation'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'
import { DLX_DEFAULT_OPTS as DEFAULT_OPTS } from './utils'

const testOnWindowsOnly = process.platform === 'win32' ? test : test.skip

function sanitizeDlxCacheComponent (cacheName: string): string {
  if (cacheName === 'pkg') return cacheName
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

function verifyDlxCache (cacheName: string): void {
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', cacheName))
      .map(sanitizeDlxCacheComponent)
      .sort()
  ).toStrictEqual([
    'pkg',
    '***********-*****',
  ].sort())
  verifyDlxCacheLink(cacheName)
}

function verifyDlxCacheLink (cacheName: string): void {
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', cacheName, 'pkg'))
      .sort()
  ).toStrictEqual([
    'node_modules',
    'package.json',
    'pnpm-lock.yaml',
  ].sort())
  expect(
    path.dirname(fs.realpathSync(path.resolve('cache', 'dlx', cacheName, 'pkg')))
  ).toBe(path.resolve('cache', 'dlx', cacheName))
}

afterEach(() => {
  jest.restoreAllMocks()
})

test('dlx', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
  }, ['shx', 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx install from git', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
  }, ['shelljs/shx#0dcbb9d1022037268959f8b706e0f06a6fd43fde', 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx should work when the package name differs from the bin name', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
  }, ['@pnpm.e2e/touch-file-one-bin'])

  expect(fs.existsSync('touch.txt')).toBeTruthy()
})

test('dlx should fail when the installed package has many commands and none equals the package name', async () => {
  prepareEmpty()

  await expect(
    dlx.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('project'),
      storeDir: path.resolve('store'),
    }, ['@pnpm.e2e/touch-file-many-bins'])
  ).rejects.toThrow('Could not determine executable to run. @pnpm.e2e/touch-file-many-bins has multiple binaries: t, tt')
})

test('dlx should not fail when the installed package has many commands and one equals the package name', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
  }, ['@pnpm.e2e/touch-file-good-bin-name'])

  expect(fs.existsSync('touch.txt')).toBeTruthy()
})

test('dlx --package <pkg1> [--package <pkg2>]', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    package: [
      'zkochan/for-testing-pnpm-dlx',
      'is-positive',
    ],
  }, ['foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx should fail when the package has no bins', async () => {
  prepareEmpty()

  await expect(
    dlx.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('project'),
      storeDir: path.resolve('store'),
    }, ['is-positive'])
  ).rejects.toThrow(/No binaries found in is-positive/)
})

test('dlx should work in shell mode', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    package: [
      'is-positive',
    ],
    shellMode: true,
  }, ['echo "some text" > foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx should return a non-zero exit code when the underlying script fails', async () => {
  prepareEmpty()

  const { exitCode } = await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    package: [
      'touch@3.1.0',
    ],
  }, ['nodetouch', '--bad-option'])

  expect(exitCode).toBe(1)
})

testOnWindowsOnly('dlx should work when running in the root of a Windows Drive', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: 'C:\\',
    storeDir: path.resolve('store'),
  }, ['cowsay', 'hello'])
})

test('dlx with cache', async () => {
  prepareEmpty()

  const spy = jest.spyOn(add, 'handler')

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['shx', 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBe(true)
  verifyDlxCache(createBase32Hash('shx'))
  expect(spy).toHaveBeenCalled()

  spy.mockReset()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['shx', 'touch', 'bar'])

  expect(fs.existsSync('bar')).toBe(true)
  verifyDlxCache(createBase32Hash('shx'))
  expect(spy).not.toHaveBeenCalled()

  spy.mockRestore()
})

test('dlx does not reuse expired cache', async () => {
  prepareEmpty()

  const now = new Date()

  // first execution to initialize the cache
  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['shx', 'echo', 'hello world'])
  verifyDlxCache(createBase32Hash('shx'))

  // change the date attributes of the cache to 30 minutes older than now
  const newDate = new Date(now.getTime() - 30 * 60_000)
  fs.lutimesSync(path.resolve('cache', 'dlx', createBase32Hash('shx'), 'pkg'), newDate, newDate)

  const spy = jest.spyOn(add, 'handler')

  // main dlx execution
  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: 10, // 10 minutes should make 30 minutes old cache expired
  }, ['shx', 'touch', 'BAR'])

  expect(fs.existsSync('BAR')).toBe(true)
  expect(spy).toHaveBeenCalledWith(expect.anything(), ['shx'])

  spy.mockRestore()

  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx')))
      .map(sanitizeDlxCacheComponent)
      .sort()
  ).toStrictEqual([
    'pkg',
    '***********-*****',
    '***********-*****',
  ].sort())
  verifyDlxCacheLink(createBase32Hash('shx'))
})

test('dlx still saves cache even if execution fails', async () => {
  prepareEmpty()

  fs.writeFileSync(path.resolve('not-a-dir'), 'to make `shx mkdir` fails')

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['shx', 'mkdir', path.resolve('not-a-dir')])

  expect(fs.readFileSync(path.resolve('not-a-dir'), 'utf-8')).toEqual(expect.anything())
  verifyDlxCache(createBase32Hash('shx'))
})
