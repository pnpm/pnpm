import fs from 'fs'
import path from 'path'
import PATH_NAME from 'path-name'
import { getConfig } from '@pnpm/config'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { addUser, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { execPnpm, execPnpmSync } from './utils'

let registries: Record<string, string>

beforeAll(async () => {
  const { config } = await getConfig({ cliOptions: {}, packageManager: { name: '', version: '' } })
  registries = config.registries
  registries.default = `http://localhost:${REGISTRY_MOCK_PORT}/`
})

const createCacheKey = (...pkgs: string[]): string => dlx.createCacheKey(pkgs, registries)

test('silent dlx prints the output of the child process only', async () => {
  prepare({})
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  const result = execPnpmSync(['--silent', 'dlx', 'shx', 'echo', 'hi'], { env })

  expect(result.stdout.toString().trim()).toBe('hi')
})

test('dlx ignores configuration in current project package.json', async () => {
  prepare({
    pnpm: {
      patchedDependencies: {
        'shx@0.3.4': 'this_does_not_exist',
      },
    },
  })
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  const result = execPnpmSync(['dlx', 'shx@0.3.4', 'echo', 'hi'], { env })
  // It didn't try to use the patch that doesn't exist, so it did not fail
  expect(result.status).toBe(0)
})

test('dlx should work with npm_config_save_dev env variable', async () => {
  prepareEmpty()
  const result = execPnpmSync(['dlx', '@foo/touch-file-one-bin@latest'], {
    env: {
      npm_config_save_dev: 'true',
    },
    stdio: 'inherit',
  })
  expect(result.status).toBe(0)
})

test('parallel dlx calls of the same package', async () => {
  prepareEmpty()

  // parallel dlx calls without cache
  await Promise.all(['foo', 'bar', 'baz'].map(
    name => execPnpm([
      `--config.store-dir=${path.resolve('store')}`,
      `--config.cache-dir=${path.resolve('cache')}`,
      '--config.dlx-cache-max-age=Infinity',
      'dlx', 'shx', 'touch', name])
  ))

  expect(['foo', 'bar', 'baz'].filter(name => fs.existsSync(name))).toStrictEqual(['foo', 'bar', 'baz'])
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'), 'pkg'))
  ).toStrictEqual([
    'node_modules',
    'package.json',
    'pnpm-lock.yaml',
  ])
  expect(
    path.dirname(fs.realpathSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'), 'pkg')))
  ).toBe(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4')))

  const cacheContentAfterFirstRun = fs.readdirSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'))).sort()

  // parallel dlx calls with cache
  await Promise.all(['abc', 'def', 'ghi'].map(
    name => execPnpm(['dlx', 'shx', 'mkdir', name])
  ))

  expect(['abc', 'def', 'ghi'].filter(name => fs.existsSync(name))).toStrictEqual(['abc', 'def', 'ghi'])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'))).sort()).toStrictEqual(cacheContentAfterFirstRun)
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'), 'pkg'))
  ).toStrictEqual([
    'node_modules',
    'package.json',
    'pnpm-lock.yaml',
  ])
  expect(
    path.dirname(fs.realpathSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'), 'pkg')))
  ).toBe(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4')))

  // parallel dlx calls with expired cache
  await Promise.all(['a/b/c', 'd/e/f', 'g/h/i'].map(
    dirPath => execPnpm([
      `--config.store-dir=${path.resolve('store')}`,
      `--config.cache-dir=${path.resolve('cache')}`,
      '--config.dlx-cache-max-age=0',
      'dlx', 'shx', 'mkdir', '-p', dirPath])
  ))

  expect(['a/b/c', 'd/e/f', 'g/h/i'].filter(name => fs.existsSync(name))).toStrictEqual(['a/b/c', 'd/e/f', 'g/h/i'])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'))).length).toBeGreaterThan(cacheContentAfterFirstRun.length)
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'), 'pkg'))
  ).toStrictEqual([
    'node_modules',
    'package.json',
    'pnpm-lock.yaml',
  ])
  expect(
    path.dirname(fs.realpathSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'), 'pkg')))
  ).toBe(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4')))
})

test('dlx creates cache and store prune cleans cache', async () => {
  prepareEmpty()

  const commands = {
    shx: ['echo', 'hello from shx'],
    'shelljs/shx#61aca968cd7afc712ca61a4fc4ec3201e3770dc7': ['echo', 'hello from shx.git'],
    '@pnpm.e2e/touch-file-good-bin-name': [],
    '@pnpm.e2e/touch-file-one-bin': [],
  } satisfies Record<string, string[]>

  const settings = [
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${path.resolve('cache')}`,
    '--config.dlx-cache-max-age=50', // big number to avoid false negative should test unexpectedly takes too long to run
  ]

  await Promise.all(Object.entries(commands).map(([cmd, args]) => execPnpm([...settings, 'dlx', cmd, ...args])))

  // ensure that the dlx cache has certain structure
  const dlxBaseDir = path.resolve('cache', 'dlx')
  const dlxDirs = fs.readdirSync(dlxBaseDir)
  expect(dlxDirs.length).toEqual(Object.keys(commands).length)
  for (const dlxDir of dlxDirs) {
    expect(fs.readdirSync(path.resolve(dlxBaseDir, dlxDir)).length).toBe(2)
  }

  // modify the dates of the cache items
  const ageTable = {
    [dlxDirs[0]]: 20,
    [dlxDirs[1]]: 75,
    [dlxDirs[2]]: 33,
    [dlxDirs[3]]: 123,
  } satisfies Record<string, number>
  const now = new Date()
  await Promise.all(Object.entries(ageTable).map(async ([dlxDir, age]) => {
    const newDate = new Date(now.getTime() - age * 60_000)
    const dlxCacheLink = path.resolve('cache', 'dlx', dlxDir, 'pkg')
    await fs.promises.lutimes(dlxCacheLink, newDate, newDate)
  }))

  await execPnpm([...settings, 'store', 'prune'])

  // test to see if dlx cache items are deleted or kept as expected
  const keptDirs = [dlxDirs[0], dlxDirs[2]].sort()
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx')).sort()
  ).toStrictEqual(keptDirs)
  for (const keptDir of keptDirs) {
    expect(fs.readdirSync(path.resolve('cache', 'dlx', keptDir)).length).toBe(2)
  }

  await execPnpm([
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${path.resolve('cache')}`,
    '--config.dlx-cache-max-age=0',
    'store', 'prune'])

  // test to see if all dlx cache items are deleted
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx'))
      .sort()
  ).toStrictEqual([])
})

test('dlx should ignore non-auth info from .npmrc in the current directory', async () => {
  prepare({})
  fs.writeFileSync('.npmrc', 'hoist-pattern=', 'utf8')

  const cacheDir = path.resolve('cache')
  await execPnpm([
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${cacheDir}`,
    'dlx', 'shx', 'echo', 'hi'])

  const modulesManifest = await readModulesManifest(path.join(cacheDir, 'dlx', createCacheKey('shx@0.3.4'), 'pkg/node_modules'))
  expect(modulesManifest?.hoistPattern).toStrictEqual(['*'])
})

test('dlx read registry from .npmrc in the current directory', async () => {
  prepareEmpty()

  const data = await addUser({
    email: 'foo@bar.com',
    password: 'bar',
    username: 'foo',
  })

  fs.writeFileSync('.npmrc', [
    `registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
    `//localhost:${REGISTRY_MOCK_PORT}/:_authToken=${data.token}`,
  ].join('\n'))

  const execResult = execPnpmSync([
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${path.resolve('cache')}`,
    '--package=@pnpm.e2e/needs-auth',
    'dlx',
    'hello-from-needs-auth',
  ], {
    env: {},
    stdio: [null, 'pipe', 'inherit'],
  })

  expect(execResult.stdout.toString().trim()).toBe('hello from @pnpm.e2e/needs-auth')
  expect(execResult.status).toBe(0)
})

test('dlx uses the node version specified by --use-node-version', async () => {
  prepareEmpty()

  const pnpmHome = path.resolve('home')

  const execResult = execPnpmSync([
    '--use-node-version=20.0.0',
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${path.resolve('cache')}`,
    'dlx',
    '@pnpm.e2e/print-node-info',
  ], {
    env: {
      PNPM_HOME: pnpmHome,
    },
    stdio: [null, 'pipe', 'inherit'],
  })

  if (execResult.status !== 0) {
    console.error(execResult.stderr.toString())
    throw new Error(`Process exits with code ${execResult.status}`)
  }

  let nodeInfo
  try {
    nodeInfo = JSON.parse(execResult.stdout.toString())
  } catch (err) {
    console.error(execResult.stdout.toString())
    console.error(execResult.stderr.toString())
    throw err
  }

  expect(nodeInfo).toMatchObject({
    versions: {
      node: '20.0.0',
    },
    execPath: process.platform === 'win32'
      ? path.join(pnpmHome, 'nodejs', '20.0.0', 'node.exe')
      : path.join(pnpmHome, 'nodejs', '20.0.0', 'bin', 'node'),
  })

  expect(execResult.status).toBe(0)
})
