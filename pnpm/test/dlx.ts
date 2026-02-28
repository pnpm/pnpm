import fs from 'fs'
import path from 'path'
import PATH_NAME from 'path-name'
import { getConfig } from '@pnpm/config'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { addUser, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { type BaseManifest } from '@pnpm/types'
import { execPnpm, execPnpmSync } from './utils/index.js'

let registries: Record<string, string>

beforeAll(async () => {
  const { config } = await getConfig({ cliOptions: {}, packageManager: { name: '', version: '' } })
  registries = config.registries
  registries.default = `http://localhost:${REGISTRY_MOCK_PORT}/`
})

const createCacheKey = (...packages: string[]): string => dlx.createCacheKey({ packages, registries })

const describeOnLinuxOnly = process.platform === 'linux' ? describe : describe.skip

test('dlx parses options between "dlx" and the command name', async () => {
  prepareEmpty()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  const result = execPnpmSync(['dlx', '--package', 'shx@0.3.4', '--silent', 'shx', 'echo', 'hi'], { env, expectSuccess: true })

  expect(result.stdout.toString().trim()).toBe('hi')
})

test('silent dlx prints the output of the child process only', async () => {
  prepareEmpty()
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  const result = execPnpmSync(['--silent', 'dlx', 'shx@0.3.4', 'echo', 'hi'], { env, expectSuccess: true })

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

  execPnpmSync(['dlx', 'shx@0.3.4', 'echo', 'hi'], {
    env,
    expectSuccess: true, // It didn't try to use the patch that doesn't exist, so it did not fail
  })
})

test('dlx should work with npm_config_save_dev env variable', async () => {
  prepareEmpty()
  execPnpmSync(['dlx', '@foo/touch-file-one-bin@latest'], {
    env: {
      npm_config_save_dev: 'true',
    },
    stdio: 'inherit',
    expectSuccess: true,
  })
})

test('parallel dlx calls of the same package', async () => {
  prepareEmpty()

  // parallel dlx calls without cache
  await Promise.all(['foo', 'bar', 'baz'].map(
    name => execPnpm([
      `--config.store-dir=${path.resolve('store')}`,
      `--config.cache-dir=${path.resolve('cache')}`,
      '--config.dlx-cache-max-age=Infinity',
      'dlx', 'shx@0.3.4', 'touch', name])
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
    name => execPnpm(['dlx', 'shx@0.3.4', 'mkdir', name])
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
      'dlx', 'shx@0.3.4', 'mkdir', '-p', dirPath])
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

  await Promise.all(Object.entries(commands).map(([cmd, args]) => execPnpm([...settings, '--allow-build=shx', 'dlx', cmd, ...args])))

  // ensure that the dlx cache has certain structure
  const dlxBaseDir = path.resolve('cache', 'dlx')
  const dlxDirs = fs.readdirSync(dlxBaseDir)
  expect(dlxDirs).toHaveLength(Object.keys(commands).length)
  for (const dlxDir of dlxDirs) {
    expect(fs.readdirSync(path.resolve(dlxBaseDir, dlxDir))).toHaveLength(2)
  }

  // modify the dates of the cache items
  const ageTable = {
    [dlxDirs[0]]: 20,
    [dlxDirs[1]]: 75,
    [dlxDirs[2]]: 33,
    [dlxDirs[3]]: 123,
  } satisfies Record<string, number>
  const now = new Date()
  Object.entries(ageTable).forEach(([dlxDir, age]) => {
    const newDate = new Date(now.getTime() - age * 60_000)
    const dlxCacheLink = path.resolve('cache', 'dlx', dlxDir, 'pkg')
    fs.lutimesSync(dlxCacheLink, newDate, newDate)
  })

  await execPnpm([...settings, 'store', 'prune'])

  // test to see if dlx cache items are deleted or kept as expected
  const keptDirs = [dlxDirs[0], dlxDirs[2]].sort()
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx')).sort()
  ).toStrictEqual(keptDirs)
  for (const keptDir of keptDirs) {
    expect(fs.readdirSync(path.resolve('cache', 'dlx', keptDir))).toHaveLength(2)
  }

  await execPnpm([
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${path.resolve('cache')}`,
    '--config.dlx-cache-max-age=0',
    'store', 'prune'])

  // test to see if all dlx cache items are deleted
  expect(fs.readdirSync(path.resolve('cache', 'dlx'))).toStrictEqual([])
})

test('dlx should ignore non-auth info from .npmrc in the current directory', async () => {
  prepareEmpty()
  fs.writeFileSync('.npmrc', 'hoist-pattern=', 'utf8')

  const cacheDir = path.resolve('cache')
  await execPnpm([
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${cacheDir}`,
    'dlx', 'shx@0.3.4', 'echo', 'hi'])

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
    expectSuccess: true,
  })

  expect(execResult.stdout.toString().trim()).toBe('hello from @pnpm.e2e/needs-auth')
})

test('dlx uses the node version specified by --package=node@runtime:<version>', async () => {
  prepareEmpty()

  const pnpmHome = path.resolve('home')

  const execResult = execPnpmSync([
    '--package=node@runtime:20.0.0',
    '--package=@pnpm.e2e/print-node-info',
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${path.resolve('cache')}`,
    'dlx',
    'print-node-info',
  ], {
    env: {
      PNPM_HOME: pnpmHome,
    },
    stdio: [null, 'pipe', 'inherit'],
    expectSuccess: true,
  })

  let nodeInfo
  try {
    nodeInfo = JSON.parse(execResult.stdout.toString())
  } catch (err) {
    console.error(execResult.stdout.toString())
    console.error(execResult.stderr.toString())
    throw err
  }

  expect(nodeInfo.versions.node).toBe('20.0.0')
  // On Windows, node.exe is hardlinked into .bin/ so process.execPath
  // reports the hardlink path rather than the original store location.
  // On non-Windows, the symlink is resolved by the kernel.
  if (process.platform !== 'win32') {
    expect(nodeInfo.execPath).toContain(path.normalize('links/@/node/20.0.0'))
  }
})

test('dlx without arguments prints help text and exits with 1', () => {
  prepareEmpty()

  const result = execPnpmSync(['dlx'])

  expect(result.status).toBe(1)

  const output = result.stdout.toString()
  expect(output).toMatch(/Run a package in a temporary environment\./)
})

describeOnLinuxOnly('dlx with supportedArchitectures CLI options', () => {
  type CPU = 'arm64' | 'x64'
  type LibC = 'glibc' | 'musl'
  type OS = 'darwin' | 'linux' | 'win32'
  type CLIOption = `--cpu=${CPU}` | `--libc=${LibC}` | `--os=${OS}`
  type Installed = string[]
  type NotInstalled = string[]
  type Case = [CLIOption[], Installed, NotInstalled]

  test.each([
    [['--cpu=arm64', '--os=win32'], ['@pnpm.e2e/only-win32-arm64'], [
      '@pnpm.e2e/only-darwin-arm64',
      '@pnpm.e2e/only-darwin-x64',
      '@pnpm.e2e/only-linux-arm64-glibc',
      '@pnpm.e2e/only-linux-arm64-musl',
      '@pnpm.e2e/only-linux-x64-glibc',
      '@pnpm.e2e/only-linux-x64-musl',
      '@pnpm.e2e/only-win32-x64',
    ]],

    [['--cpu=arm64', '--os=darwin'], ['@pnpm.e2e/only-darwin-arm64'], [
      '@pnpm.e2e/only-darwin-x64',
      '@pnpm.e2e/only-linux-arm64-glibc',
      '@pnpm.e2e/only-linux-arm64-musl',
      '@pnpm.e2e/only-linux-x64-glibc',
      '@pnpm.e2e/only-linux-x64-musl',
      '@pnpm.e2e/only-win32-arm64',
      '@pnpm.e2e/only-win32-x64',
    ]],

    [['--cpu=x64', '--os=linux', '--libc=musl'], [
      '@pnpm.e2e/only-linux-x64-musl',
    ], [
      '@pnpm.e2e/only-darwin-arm64',
      '@pnpm.e2e/only-darwin-x64',
      '@pnpm.e2e/only-linux-arm64-glibc',
      '@pnpm.e2e/only-linux-arm64-musl',
      '@pnpm.e2e/only-linux-x64-glibc',
      '@pnpm.e2e/only-win32-arm64',
      '@pnpm.e2e/only-win32-x64',
    ]],

    [[
      '--cpu=arm64',
      '--cpu=x64',
      '--os=darwin',
      '--os=linux',
      '--os=win32',
    ], [
      '@pnpm.e2e/only-darwin-arm64',
      '@pnpm.e2e/only-darwin-x64',
      '@pnpm.e2e/only-linux-arm64-glibc',
      '@pnpm.e2e/only-linux-x64-glibc',
      '@pnpm.e2e/only-win32-arm64',
      '@pnpm.e2e/only-win32-x64',
    ], [
      '@pnpm.e2e/only-linux-arm64-musl',
      '@pnpm.e2e/only-linux-x64-musl',
    ]],
  ] as Case[])('%p', async (cliOpts, installed, notInstalled) => {
    prepareEmpty()

    const execResult = execPnpmSync([
      `--config.store-dir=${path.resolve('store')}`,
      `--config.cache-dir=${path.resolve('cache')}`,
      '--package=@pnpm.e2e/support-different-architectures',
      ...cliOpts,
      'dlx',
      'get-optional-dependencies',
    ], {
      stdio: [null, 'pipe', 'inherit'],
      expectSuccess: true,
    })

    interface OptionalDepsInfo {
      installed: Record<string, BaseManifest>
      notInstalled: string[]
    }

    let optionalDepsInfo: OptionalDepsInfo
    try {
      optionalDepsInfo = JSON.parse(execResult.stdout.toString())
    } catch (err) {
      console.error(execResult.stdout.toString())
      console.error(execResult.stderr.toString())
      throw err
    }

    expect(optionalDepsInfo).toStrictEqual({
      installed: Object.fromEntries(installed.map(name => [name, expect.objectContaining({ name })])),
      notInstalled,
    } as OptionalDepsInfo)
  })
})
