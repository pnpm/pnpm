import fs from 'fs'
import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { jest } from '@jest/globals'
import { DLX_DEFAULT_OPTS as DEFAULT_OPTS } from './utils/index.js'

const { getSystemNodeVersion: originalGetSystemNodeVersion } = await import('@pnpm/env.system-node-version')
jest.unstable_mockModule('@pnpm/env.system-node-version', () => ({
  getSystemNodeVersion: jest.fn(originalGetSystemNodeVersion),
}))
const { installGlobalPackages: originalInstallGlobalPackages } = await import('@pnpm/global.commands')
jest.unstable_mockModule('@pnpm/global.commands', () => ({
  installGlobalPackages: jest.fn(originalInstallGlobalPackages),
}))
const { rebuild: originalRebuild } = await import('@pnpm/plugin-commands-rebuild')
jest.unstable_mockModule('@pnpm/plugin-commands-rebuild', () => ({
  rebuild: {
    ...originalRebuild,
    handler: jest.fn(originalRebuild.handler),
  },
}))
const { approveBuilds: originalApproveBuilds } = await import('@pnpm/exec.build-commands')
jest.unstable_mockModule('@pnpm/exec.build-commands', () => ({
  approveBuilds: {
    ...originalApproveBuilds,
    handler: jest.fn(originalApproveBuilds.handler),
  },
}))

const systemNodeVersion = await import('@pnpm/env.system-node-version')
const { installGlobalPackages } = await import('@pnpm/global.commands')
const { rebuild } = await import('@pnpm/plugin-commands-rebuild')
const { approveBuilds } = await import('@pnpm/exec.build-commands')
const { dlx } = await import('@pnpm/plugin-commands-script-runners')

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

const createCacheKey = (...packages: string[]): string => dlx.createCacheKey({
  packages,
  registries: DEFAULT_OPTS.registries,
  supportedArchitectures: DEFAULT_OPTS.supportedArchitectures,
})

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
  const files = fs.readdirSync(path.resolve('cache', 'dlx', cacheName, 'pkg')).sort()
  // pnpm-workspace.yaml is created when allowBuilds state is persisted
  expect(files).toContain('node_modules')
  expect(files).toContain('package.json')
  expect(files).toContain('pnpm-lock.yaml')
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
  }, ['shx@0.3.4', 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx install from git', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    allowBuild: ['shx'],
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
      '@pnpm.e2e/for-testing-pnpm-dlx',
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

test('dlx should work when symlink=false', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    symlink: false,
  }, ['@pnpm.e2e/touch-file-good-bin-name'])

  expect(fs.existsSync('touch.txt')).toBeTruthy()
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

  const spy = jest.mocked(installGlobalPackages)

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['shx@0.3.4', 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBe(true)
  verifyDlxCache(createCacheKey('shx@0.3.4'))
  expect(spy).toHaveBeenCalled()

  spy.mockClear()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['shx@0.3.4', 'touch', 'bar'])

  expect(fs.existsSync('bar')).toBe(true)
  verifyDlxCache(createCacheKey('shx@0.3.4'))
  expect(spy).not.toHaveBeenCalled()

  spy.mockClear()

  // Specify a node version that shx@0.3.4 does not support. Currently supported versions are >= 6.
  jest.mocked(systemNodeVersion.getSystemNodeVersion).mockReturnValue('v4.0.0')

  await expect(dlx.handler({
    ...DEFAULT_OPTS,
    engineStrict: true,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['shx@0.3.4', 'touch', 'foo'])).rejects.toThrow('Unsupported engine for')

  jest.mocked(systemNodeVersion.getSystemNodeVersion).mockImplementation(originalGetSystemNodeVersion)
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
  }, ['shx@0.3.4', 'echo', 'hello world'])
  verifyDlxCache(createCacheKey('shx@0.3.4'))

  // change the date attributes of the cache to 30 minutes older than now
  const newDate = new Date(now.getTime() - 30 * 60_000)
  fs.lutimesSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4'), 'pkg'), newDate, newDate)

  const spy = jest.mocked(installGlobalPackages)

  // main dlx execution
  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: 10, // 10 minutes should make 30 minutes old cache expired
  }, ['shx@0.3.4', 'touch', 'BAR'])

  expect(fs.existsSync('BAR')).toBe(true)
  expect(spy).toHaveBeenCalledWith(expect.anything(), ['shx@0.3.4'])

  spy.mockClear()

  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', createCacheKey('shx@0.3.4')))
      .map(sanitizeDlxCacheComponent)
      .sort()
  ).toStrictEqual([
    'pkg',
    '***********-*****',
    '***********-*****',
  ].sort())
  verifyDlxCacheLink(createCacheKey('shx@0.3.4'))
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
  }, ['shx@0.3.4', 'mkdir', path.resolve('not-a-dir')])

  expect(fs.readFileSync(path.resolve('not-a-dir'), 'utf-8')).toEqual(expect.anything())
  verifyDlxCache(createCacheKey('shx@0.3.4'))
})

test('dlx builds the package that is executed', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['@pnpm.e2e/has-bin-and-needs-build'])

  // The command file of the above package is created by a postinstall script
  // so if it doesn't fail it means that it was built.

  const dlxCacheDir = path.resolve('cache', 'dlx', createCacheKey('@pnpm.e2e/has-bin-and-needs-build@1.0.0'), 'pkg')
  const builtPkg1Path = path.join(dlxCacheDir, 'node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0/node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')
  expect(fs.existsSync(path.join(builtPkg1Path, 'package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(builtPkg1Path, 'generated-by-preinstall.js'))).toBeFalsy()
  expect(fs.existsSync(path.join(builtPkg1Path, 'generated-by-postinstall.js'))).toBeFalsy()

  const builtPkg2Path = path.join(dlxCacheDir, 'node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0/node_modules/@pnpm.e2e/install-script-example')
  expect(fs.existsSync(path.join(builtPkg2Path, 'package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(builtPkg2Path, 'generated-by-install.js'))).toBeFalsy()
})

test('dlx builds the packages passed via --allow-build', async () => {
  prepareEmpty()

  const allowBuild = ['@pnpm.e2e/install-script-example']
  await dlx.handler({
    ...DEFAULT_OPTS,
    allowBuild,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['@pnpm.e2e/has-bin-and-needs-build'])

  const dlxCacheDir = path.resolve('cache', 'dlx', dlx.createCacheKey({
    packages: ['@pnpm.e2e/has-bin-and-needs-build@1.0.0'],
    registries: DEFAULT_OPTS.registries,
    supportedArchitectures: DEFAULT_OPTS.supportedArchitectures,
  }), 'pkg')
  const builtPkg1Path = path.join(dlxCacheDir, 'node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0/node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')
  expect(fs.existsSync(path.join(builtPkg1Path, 'package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(builtPkg1Path, 'generated-by-preinstall.js'))).toBeFalsy()
  expect(fs.existsSync(path.join(builtPkg1Path, 'generated-by-postinstall.js'))).toBeFalsy()

  const builtPkg2Path = path.join(dlxCacheDir, 'node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0/node_modules/@pnpm.e2e/install-script-example')
  expect(fs.existsSync(path.join(builtPkg2Path, 'package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(builtPkg2Path, 'generated-by-install.js'))).toBeTruthy()
})

test('dlx should fail when the requested package does not meet the minimum age requirement', async () => {
  prepareEmpty()

  await expect(
    dlx.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('project'),
      minimumReleaseAge: 60 * 24 * 10000,
      registries: {
        // We must use the public registry instead of verdaccio here
        // because verdaccio has the "times" field in the abbreviated metadata too.
        default: 'https://registry.npmjs.org/',
      },
    }, ['shx@0.3.4'])
  ).rejects.toThrow(/Version 0\.3\.4 \(released .+\) of shx does not meet the minimumReleaseAge constraint/)
})

test('dlx should respect minimumReleaseAgeExclude', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    minimumReleaseAge: 60 * 24 * 10000,
    minimumReleaseAgeExclude: ['*'],
    registries: {
      // We must use the public registry instead of verdaccio here
      // because verdaccio has the "times" field in the abbreviated metadata too.
      default: 'https://registry.npmjs.org/',
    },
  }, ['shx@0.3.4', 'touch', 'foo'])

  expect(fs.existsSync('foo')).toBeTruthy()
})

test('dlx with catalog', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
    catalogs: {
      default: {
        shx: '^0.3.4',
      },
    },
  }, ['shx@catalog:'])

  verifyDlxCache(createCacheKey('shx@0.3.4'))
})

test('dlx calls approveBuilds.handler when builds are ignored in TTY mode', async () => {
  prepareEmpty()

  const installSpy = jest.mocked(installGlobalPackages)
  const approveSpy = jest.mocked(approveBuilds.handler)

  // Wrap installGlobalPackages to call original but return ignoredBuilds
  installSpy.mockImplementationOnce(async (...args) => {
    await originalInstallGlobalPackages(...args)
    return new Set(['@pnpm.e2e/some-package'])
  })

  // Mock TTY
  const originalIsTTY = process.stdin.isTTY
  Object.defineProperty(process.stdin, 'isTTY', { value: true, configurable: true })

  // Mock approveBuilds.handler to be a no-op (avoid actual interactive prompts)
  approveSpy.mockResolvedValueOnce(undefined)

  try {
    await dlx.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('project'),
      storeDir: path.resolve('store'),
      cacheDir: path.resolve('cache'),
      dlxCacheMaxAge: Infinity,
    }, ['shx@0.3.4', 'touch', 'foo'])

    expect(approveSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        global: false,
        pending: false,
      })
    )
  } finally {
    Object.defineProperty(process.stdin, 'isTTY', { value: originalIsTTY, configurable: true })
  }
})

test('dlx does not call approveBuilds.handler in non-TTY mode', async () => {
  prepareEmpty()

  const approveSpy = jest.mocked(approveBuilds.handler)
  approveSpy.mockClear()

  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['@pnpm.e2e/has-bin-and-needs-build'])

  expect(approveSpy).not.toHaveBeenCalled()
})

test('dlx persists allowBuilds to pnpm-workspace.yaml on cache miss', async () => {
  prepareEmpty()

  await dlx.handler({
    ...DEFAULT_OPTS,
    allowBuild: ['@pnpm.e2e/install-script-example'],
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['@pnpm.e2e/has-bin-and-needs-build'])

  const dlxCacheDir = path.resolve('cache', 'dlx', createCacheKey('@pnpm.e2e/has-bin-and-needs-build@1.0.0'), 'pkg')
  const workspaceYaml = fs.readFileSync(path.join(dlxCacheDir, 'pnpm-workspace.yaml'), 'utf8')
  expect(workspaceYaml).toContain('@pnpm.e2e/has-bin-and-needs-build')
  expect(workspaceYaml).toContain('@pnpm.e2e/install-script-example')
})

test('dlx cache hit with same --allow-build reuses cache without rebuild', async () => {
  prepareEmpty()

  const installSpy = jest.mocked(installGlobalPackages)
  const rebuildSpy = jest.mocked(rebuild.handler)

  const commonOpts = {
    ...DEFAULT_OPTS,
    allowBuild: ['@pnpm.e2e/install-script-example'],
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }

  // First run: cache miss
  await dlx.handler(commonOpts, ['@pnpm.e2e/has-bin-and-needs-build'])
  expect(installSpy).toHaveBeenCalled()

  installSpy.mockClear()
  rebuildSpy.mockClear()

  // Second run: cache hit with same allowBuild
  await dlx.handler(commonOpts, ['@pnpm.e2e/has-bin-and-needs-build'])
  expect(installSpy).not.toHaveBeenCalled()
  expect(rebuildSpy).not.toHaveBeenCalled()
})

test('dlx cache hit with new --allow-build triggers rebuild', async () => {
  prepareEmpty()

  const installSpy = jest.mocked(installGlobalPackages)
  const rebuildSpy = jest.mocked(rebuild.handler)

  // First run: no --allow-build
  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['@pnpm.e2e/has-bin-and-needs-build'])
  expect(installSpy).toHaveBeenCalled()

  // Verify the package is NOT built yet
  const dlxCacheDir = path.resolve('cache', 'dlx', createCacheKey('@pnpm.e2e/has-bin-and-needs-build@1.0.0'), 'pkg')
  const builtPkgPath = path.join(dlxCacheDir, 'node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0/node_modules/@pnpm.e2e/install-script-example')
  expect(fs.existsSync(path.join(builtPkgPath, 'generated-by-install.js'))).toBeFalsy()

  installSpy.mockClear()
  rebuildSpy.mockClear()

  // Second run: add --allow-build for a dependency
  await dlx.handler({
    ...DEFAULT_OPTS,
    allowBuild: ['@pnpm.e2e/install-script-example'],
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['@pnpm.e2e/has-bin-and-needs-build'])

  // Should NOT reinstall (cache hit)
  expect(installSpy).not.toHaveBeenCalled()
  // Should call rebuild for the newly allowed package
  expect(rebuildSpy).toHaveBeenCalledWith(
    expect.anything(),
    ['@pnpm.e2e/install-script-example']
  )

  // Verify the package IS now built
  expect(fs.existsSync(path.join(builtPkgPath, 'generated-by-install.js'))).toBeTruthy()
})

test('dlx cache hit with removed --allow-build triggers cache invalidation', async () => {
  prepareEmpty()

  const installSpy = jest.mocked(installGlobalPackages)

  // First run: with --allow-build
  await dlx.handler({
    ...DEFAULT_OPTS,
    allowBuild: ['@pnpm.e2e/install-script-example'],
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['@pnpm.e2e/has-bin-and-needs-build'])
  expect(installSpy).toHaveBeenCalled()

  // Verify the package was built
  const cacheKeyHash = createCacheKey('@pnpm.e2e/has-bin-and-needs-build@1.0.0')
  const dlxCacheDir = path.resolve('cache', 'dlx', cacheKeyHash, 'pkg')
  const builtPkgPath = path.join(dlxCacheDir, 'node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0/node_modules/@pnpm.e2e/install-script-example')
  expect(fs.existsSync(path.join(builtPkgPath, 'generated-by-install.js'))).toBeTruthy()

  installSpy.mockClear()

  // Second run: without --allow-build (removed permission)
  await dlx.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project'),
    storeDir: path.resolve('store'),
    cacheDir: path.resolve('cache'),
    dlxCacheMaxAge: Infinity,
  }, ['@pnpm.e2e/has-bin-and-needs-build'])

  // Should reinstall (cache invalidated due to removed permission)
  expect(installSpy).toHaveBeenCalledWith(expect.anything(), ['@pnpm.e2e/has-bin-and-needs-build@1.0.0'])

  // Verify cache now points to a new directory
  const newDlxCacheDir = fs.realpathSync(path.resolve('cache', 'dlx', cacheKeyHash, 'pkg'))
  expect(newDlxCacheDir).not.toBe(path.resolve(dlxCacheDir))

  // The new cache should NOT have the previously built package
  const newBuiltPkgPath = path.join(newDlxCacheDir, 'node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0/node_modules/@pnpm.e2e/install-script-example')
  expect(fs.existsSync(path.join(newBuiltPkgPath, 'generated-by-install.js'))).toBeFalsy()
})
