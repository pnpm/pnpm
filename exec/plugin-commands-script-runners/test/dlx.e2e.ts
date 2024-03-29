import fs from 'fs'
import path from 'path'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { add } from '@pnpm/plugin-commands-installation'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { prepareEmpty } from '@pnpm/prepare'
import { DLX_DEFAULT_OPTS as DEFAULT_OPTS } from './utils'

const testOnWindowsOnly = process.platform === 'win32' ? test : test.skip

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
    dlxCacheMaxAge: Infinity,
  }, ['shx', 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
  expect(fs.readdirSync(path.resolve('cache', 'dlx'))).toStrictEqual([createBase32Hash('shx')])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'))).sort()).toStrictEqual(['node_modules', 'package.json', 'pnpm-lock.yaml'])
})

test('dlx install from git', async () => {
  prepareEmpty()

  const pkg = 'shelljs/shx#61aca968cd7afc712ca61a4fc4ec3201e3770dc7'

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, [pkg, 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
  expect(fs.readdirSync(path.resolve('cache', 'dlx'))).toStrictEqual([createBase32Hash(pkg)])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash(pkg))).sort()).toStrictEqual(['node_modules', 'package.json', 'pnpm-lock.yaml'])
})

test('dlx should work when the package name differs from the bin name', async () => {
  prepareEmpty()

  const pkg = '@pnpm.e2e/touch-file-one-bin'

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, [pkg])

  expect(fs.existsSync('touch.txt')).toBeTruthy()
  expect(fs.readdirSync(path.resolve('cache', 'dlx'))).toStrictEqual([createBase32Hash(pkg)])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash(pkg))).sort()).toStrictEqual(['node_modules', 'package.json', 'pnpm-lock.yaml'])
})

test('dlx should fail when the installed package has many commands and none equals the package name', async () => {
  prepareEmpty()

  await expect(
    dlx.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('project'),
      storeDir: path.resolve('store'),
      dlxCacheMaxAge: Infinity,
    }, ['@pnpm.e2e/touch-file-many-bins'])
  ).rejects.toThrow('Could not determine executable to run. @pnpm.e2e/touch-file-many-bins has multiple binaries: t, tt')
})

test('dlx should not fail when the installed package has many commands and one equals the package name', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    dlxCacheMaxAge: Infinity,
  }, ['@pnpm.e2e/touch-file-good-bin-name'])

  expect(fs.existsSync('touch.txt')).toBeTruthy()
})

test('dlx --package <pkg1> [--package <pkg2>]', async () => {
  prepareEmpty()

  const pkgs = [
    'zkochan/for-testing-pnpm-dlx',
    'is-positive',
  ]

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    package: pkgs,
    dlxCacheMaxAge: Infinity,
  }, ['foo'])

  const cacheName = createBase32Hash(pkgs.join('\n'))
  expect(fs.readdirSync(path.resolve('cache', 'dlx'))).toStrictEqual([cacheName])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', cacheName)).sort()).toStrictEqual(['node_modules', 'package.json', 'pnpm-lock.yaml'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx should fail when the package has no bins', async () => {
  prepareEmpty()

  await expect(
    dlx.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('project'),
      storeDir: path.resolve('store'),
      dlxCacheMaxAge: Infinity,
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
    dlxCacheMaxAge: Infinity,
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
    dlxCacheMaxAge: Infinity,
  }, ['nodetouch', '--bad-option'])

  expect(exitCode).toBe(1)
})

testOnWindowsOnly('dlx should work when running in the root of a Windows Drive', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: 'C:\\',
    storeDir: path.resolve('store'),
    dlxCacheMaxAge: Infinity,
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
  expect(fs.readdirSync(path.resolve('cache', 'dlx'))).toStrictEqual([createBase32Hash('shx')])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'))).sort()).toStrictEqual(['node_modules', 'package.json', 'pnpm-lock.yaml'])
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
  expect(fs.readdirSync(path.resolve('cache', 'dlx'))).toStrictEqual([createBase32Hash('shx')])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'))).sort()).toStrictEqual(['node_modules', 'package.json', 'pnpm-lock.yaml'])
  expect(spy).not.toHaveBeenCalled()

  spy.mockRestore()
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
  expect(fs.readdirSync(path.resolve('cache', 'dlx'))).toStrictEqual([createBase32Hash('shx')])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'))).sort()).toStrictEqual(['node_modules', 'package.json', 'pnpm-lock.yaml'])
})

test('dlx installs and runs multiple instances of the same package in parallel', async () => {
  prepareEmpty()

  await Promise.all(['foo', 'bar', 'baz'].map(
    name => dlx.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('project'),
      storeDir: path.resolve('store'),
      cacheDir: path.resolve('cache'),
      dlxCacheMaxAge: Infinity,
    }, ['shx', 'touch', name])
  ))

  expect(['foo', 'bar', 'baz'].filter(name => fs.existsSync(name))).toStrictEqual(['foo', 'bar', 'baz'])
  expect(fs.readdirSync(path.resolve('cache', 'dlx'))).toStrictEqual([createBase32Hash('shx')])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'))).sort()).toStrictEqual(['node_modules', 'package.json', 'pnpm-lock.yaml'])
})
