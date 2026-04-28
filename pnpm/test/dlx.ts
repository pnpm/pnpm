import fs from 'node:fs'
import path from 'node:path'

import { getConfig } from '@pnpm/config.reader'
import { dlx } from '@pnpm/exec.commands'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { addUser, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import type { BaseManifest } from '@pnpm/types'
import PATH_NAME from 'path-name'
import { writeYamlFileSync } from 'write-yaml-file'

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

test('dlx ignores configuration in pnpm-workspace.yaml', async () => {
  prepare()
  // Write a pnpm-workspace.yaml with a patchedDependencies that doesn't exist
  // dlx should ignore this and succeed
  fs.writeFileSync('pnpm-workspace.yaml', `
patchedDependencies:
  shx@0.3.4: this_does_not_exist.patch
`)
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

// The public npm registry is used here instead of verdaccio because verdaccio
// includes the 'time' field in abbreviated metadata, which short-circuits
// the publish-date check.
describe('minimumReleaseAge from pnpm-workspace.yaml', () => {
  // Hard-coded publish timestamps from the public npm registry.
  // The gap of ~2.3 years between 0.3.2 and 0.3.3 provides ample buffer
  // for any CI timing variance.
  const SHX_0_3_2_PUBLISHED = new Date('2018-07-11T04:13:54.318Z').getTime()
  const SHX_0_3_3_PUBLISHED = new Date('2020-10-26T05:35:14.984Z').getTime()
  const MINUTES_MS = 60 * 1000

  test('dlx fails when the requested version is younger than minimumReleaseAge', () => {
    prepare()
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: 60 * 24 * 10000, // ~27.4 years: rejects everything published recently
      minimumReleaseAgeStrict: true,
    })

    const result = execPnpmSync([
      '--config.registry=https://registry.npmjs.org/',
      'dlx', 'shx@0.3.4', 'echo', 'hi',
    ], { omitEnvDefaults: ['pnpm_config_minimum_release_age'] })

    expect(result.status).toBe(1)
    expect(result.stderr.toString()).toMatch(/does not meet the minimumReleaseAge constraint/)
  })

  test('dlx succeeds when the requested version is older than minimumReleaseAge', () => {
    prepare()
    // Cutoff 30 days after 0.3.2 was published: 0.3.2 is "mature". Anything
    // newer (like 0.3.3 or 0.3.4) would not be, but the spec pins 0.3.2.
    const bufferMinutes = 30 * 24 * 60
    const minimumReleaseAge = Math.floor((Date.now() - SHX_0_3_2_PUBLISHED) / MINUTES_MS) - bufferMinutes
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge,
      minimumReleaseAgeStrict: true,
    })

    execPnpmSync([
      '--config.registry=https://registry.npmjs.org/',
      'dlx', 'shx@0.3.2', 'echo', 'hi',
    ], { expectSuccess: true, omitEnvDefaults: ['pnpm_config_minimum_release_age'] })
  })

  test('dlx picks the newest version within a range that satisfies minimumReleaseAge', () => {
    prepare()
    // Cutoff positioned between 0.3.2 (2018-07-11) and 0.3.3 (2020-10-26):
    // 0.3.2 is mature, 0.3.3 and 0.3.4 are not. Range `0.3.x` should resolve to 0.3.2.
    const cutoff = (SHX_0_3_2_PUBLISHED + SHX_0_3_3_PUBLISHED) / 2
    const minimumReleaseAge = Math.floor((Date.now() - cutoff) / MINUTES_MS)
    const cacheDir = path.resolve('cache')
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge,
      minimumReleaseAgeStrict: true,
    })

    execPnpmSync([
      `--config.cache-dir=${cacheDir}`,
      `--config.store-dir=${path.resolve('store')}`,
      '--config.registry=https://registry.npmjs.org/',
      'dlx', 'shx@0.3.x', 'echo', 'hi',
    ], { expectSuccess: true, omitEnvDefaults: ['pnpm_config_minimum_release_age'] })

    // Verify the resolved version by reading the package.json installed in the dlx cache.
    const dlxDirs = fs.readdirSync(path.resolve(cacheDir, 'dlx'))
    expect(dlxDirs).toHaveLength(1)
    const pkgJson = JSON.parse(fs.readFileSync(
      path.resolve(cacheDir, 'dlx', dlxDirs[0], 'pkg/node_modules/shx/package.json'),
      'utf8'
    ) as string)
    expect(pkgJson.version).toBe('0.3.2')
  })
})

// pnpm create delegates to dlx, so the same inheritance applies.
test('pnpm create respects minimumReleaseAge from pnpm-workspace.yaml', () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    minimumReleaseAge: 60 * 24 * 10000, // ~27.4 years: rejects everything published recently
    minimumReleaseAgeStrict: true,
  })

  const result = execPnpmSync([
    '--config.registry=https://registry.npmjs.org/',
    'create', 'esm@1.0.18',
  ], { omitEnvDefaults: ['pnpm_config_minimum_release_age'] })

  expect(result.status).toBe(1)
  expect(result.stderr.toString()).toMatch(/does not meet the minimumReleaseAge constraint/)
})

test('dlx should work with pnpm_config_save_dev env variable', async () => {
  prepareEmpty()
  execPnpmSync(['dlx', '@foo/touch-file-one-bin@latest'], {
    env: {
      pnpm_config_save_dev: 'true',
    },
    stdio: 'pipe',
    expectSuccess: true,
  })
})

const testParallel = process.version.startsWith('v25.') ? test.skip : test

testParallel('parallel dlx calls of the same package', async () => {
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
    stdio: 'pipe',
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
    stdio: 'pipe',
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
      stdio: 'pipe',
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
