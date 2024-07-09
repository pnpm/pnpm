import * as path from 'path'
import fs from 'fs'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import {
  type PackageManifestLog,
  type ProgressLog,
  type RootLog,
  type StageLog,
  type StatsLog,
} from '@pnpm/core-loggers'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { fixtures } from '@pnpm/test-fixtures'
import { type ProjectManifest, type ProjectRootDir } from '@pnpm/types'
import { addDistTag, getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
  install,
  mutateModulesInSingleProject,
  UnexpectedStoreError,
  UnexpectedVirtualStoreDirError,
} from '@pnpm/core'
import { sync as rimraf } from '@zkochan/rimraf'
import execa from 'execa'
import { isCI } from 'ci-info'
import isWindows from 'is-windows'
import semver from 'semver'
import sinon from 'sinon'
import deepRequireCwd from 'deep-require-cwd'
import { sync as writeYamlFile } from 'write-yaml-file'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)
const IS_WINDOWS = isWindows()

const testOnNonWindows = IS_WINDOWS ? test.skip : test

test('spec not specified in package.json.dependencies', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      'is-positive': '',
    },
  }, testDefaults())

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['is-positive'].specifier).toBe('')
})

test.skip('ignoring some files in the dependency', async () => {
  prepareEmpty()

  const ignoreFile = (filename: string) => filename === 'readme.md'
  await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults({}, {}, { ignoreFile }))

  // package.json was not ignored
  expect(fs.existsSync(path.resolve('node_modules', 'is-positive', 'package.json'))).toBeTruthy()
  // readme.md was ignored
  expect(fs.existsSync(path.resolve('node_modules', 'is-positive', 'readme.md'))).toBeFalsy()
})

test('no dependencies (lodash)', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await addDistTag({ package: 'lodash', version: '4.1.0', distTag: 'latest' })

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['lodash@4.0.0'],
    testDefaults({ fastUnpack: false, reporter })
  )

  expect(reporter.withArgs(sinon.match({
    initial: { name: 'project', version: '0.0.0' },
    level: 'debug',
    name: 'pnpm:package-manifest',
  } as PackageManifestLog)).callCount).toBe(1)
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix: process.cwd(),
    stage: 'resolution_started',
  } as StageLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix: process.cwd(),
    stage: 'resolution_done',
  } as StageLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix: process.cwd(),
    stage: 'importing_started',
  } as StageLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix: process.cwd(),
    stage: 'importing_done',
  } as StageLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: 1,
    level: 'debug',
    name: 'pnpm:stats',
    prefix: process.cwd(),
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix: process.cwd(),
    removed: 0,
  } as StatsLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      latest: '4.1.0',
      name: 'lodash',
      realName: 'lodash',
      version: '4.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
    prefix: process.cwd(),
  } as RootLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: {
      dependencies: {
        lodash: '4.0.0',
      },
      name: 'project',
      version: '0.0.0',
    } as ProjectManifest,
  } as PackageManifestLog)).toBeTruthy()

  const m = project.requireModule('lodash')
  expect(typeof m).toBe('function')
  expect(typeof m.clone).toBe('function')
})

test('only the new packages are added', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['@pnpm/x'], testDefaults())
  const reporter = sinon.spy()
  await addDependenciesToPackage(manifest, ['@pnpm/y'], testDefaults({ reporter }))

  expect(reporter.calledWithMatch({
    added: 1,
    level: 'debug',
    name: 'pnpm:stats',
  } as StatsLog)).toBeTruthy()
})

test('scoped modules without version spec', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@zkochan/foo'], testDefaults())

  project.has('@zkochan/foo')
})

test('scoped package with custom registry', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@scoped/peer'], testDefaults({
    // setting an incorrect default registry URL
    rawConfig: {
      '@scoped:registry': `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
    registry: 'http://localhost:9999/',
  }))

  const m = project.requireModule('@scoped/peer/package.json')
  expect(m).toBeTruthy()
})

test('modules without version spec, with custom tag config', async () => {
  const project = prepareEmpty()

  const tag = 'beta'

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: tag })

  await addDependenciesToPackage({}, ['@pnpm.e2e/dep-of-pkg-with-1-dep'], testDefaults({ tag }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
})

test('modules without version spec but with a trailing @', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/dep-of-pkg-with-1-dep@'], testDefaults())

  project.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
})

test('aliased modules without version spec but with a trailing @', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['foo@npm:@pnpm.e2e/dep-of-pkg-with-1-dep@'], testDefaults())

  project.has('foo')
})

test('installing a package by specifying a specific dist-tag', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'beta' })

  await addDependenciesToPackage({}, ['@pnpm.e2e/dep-of-pkg-with-1-dep@beta'], testDefaults())

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
})

test('update a package when installing with a dist-tag', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'beta' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/dep-of-pkg-with-1-dep'], testDefaults({ targetDependenciesField: 'devDependencies' }))

  const reporter = sinon.spy()

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/dep-of-pkg-with-1-dep@beta'], testDefaults({ targetDependenciesField: 'devDependencies', reporter }))

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      dependencyType: 'dev',
      name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
      version: '100.0.0',
    },
  } as RootLog)).toBeTruthy()

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
      version: '100.1.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()

  project.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')

  expect(manifest.devDependencies!['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('^100.1.0')
})

test('scoped modules with versions', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@zkochan/foo@1.0.0'], testDefaults({ fastUnpack: false }))

  project.has('@zkochan/foo')
})

test('multiple scoped modules (@rstacruz/...)', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], testDefaults({ fastUnpack: false }))

  expect(typeof project.requireModule('@rstacruz/tap-spec')).toBe('function')
  expect(typeof project.requireModule('@rstacruz/travis-encrypt')).toBe('function')
})

test('installing a beta version of a package', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/beta-version'], testDefaults())

  expect(manifest.dependencies?.['@pnpm.e2e/beta-version']).toBe('1.0.0-beta.0')
})

test('nested scoped modules (test-pnpm-issue219 -> @zkochan/test-pnpm-issue219)', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/test-pnpm-issue219@1.0.3'], testDefaults({ fastUnpack: false }))

  const m = project.requireModule('@pnpm.e2e/test-pnpm-issue219')
  expect(m).toBe('test-pnpm-issue219,@zkochan/test-pnpm-issue219')
})

test('idempotency', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()
  const opts = testDefaults({ reporter })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], opts)

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: '@pnpm.e2e/pkg-with-1-dep',
      version: '100.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()

  reporter.resetHistory()

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], opts)

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: '@pnpm.e2e/pkg-with-1-dep',
      version: '100.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeFalsy()

  project.has('@pnpm.e2e/pkg-with-1-dep')
})

test('reporting adding root package', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], testDefaults())

  project.storeHas('flatten', '1.0.2')

  const reporter = sinon.spy()

  await addDependenciesToPackage(manifest, ['flatten@1.0.2'], testDefaults({ reporter }))

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'flatten',
      version: '1.0.2',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()
})

test('overwriting (magic-hook@2.0.0 and @0.1.0)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], testDefaults())

  project.storeHas('flatten', '1.0.2')

  await addDependenciesToPackage(manifest, ['magic-hook@0.1.0'], testDefaults())

  // flatten is not removed from store even though it is unreferenced
  // store should be pruned to have this removed
  project.storeHas('flatten', '1.0.2')

  const m = project.requireModule('magic-hook/package.json')
  expect(m.version).toBe('0.1.0')
})

test('overwriting (is-positive@3.0.0 with is-positive@latest)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['is-positive@3.0.0'], testDefaults({ save: true }))

  project.storeHas('is-positive', '3.0.0')

  const updatedManifest = await addDependenciesToPackage(manifest, ['is-positive@latest'], testDefaults({ save: true }))

  project.storeHas('is-positive', '3.1.0')
  expect(updatedManifest.dependencies?.['is-positive']).toBe('3.1.0')
})

// Covers https://github.com/pnpm/pnpm/issues/2188
test('keeping existing specs untouched when adding new dependency', async () => {
  prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({ dependencies: { '@pnpm.e2e/bar': '^100.0.0' } }, ['@pnpm.e2e/foo@100.1.0'], testDefaults())

  expect(manifest.dependencies).toStrictEqual({ '@pnpm.e2e/bar': '^100.0.0', '@pnpm.e2e/foo': '100.1.0' })
})

test('forcing', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], testDefaults({ fastUnpack: false }))

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  rimraf(distPath)

  await addDependenciesToPackage(manifest, ['magic-hook@2.0.0'], testDefaults({ fastUnpack: false, force: true }))

  const distPathExists = fs.existsSync(distPath)
  expect(distPathExists).toBeTruthy()
})

test('argumentless forcing', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], testDefaults({ fastUnpack: false }))

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  rimraf(distPath)

  await install(manifest, testDefaults({ fastUnpack: false, force: true }))

  const distPathExists = fs.existsSync(distPath)
  expect(distPathExists).toBeTruthy()
})

test('no forcing', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], testDefaults())

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  rimraf(distPath)

  await addDependenciesToPackage(manifest, ['magic-hook@2.0.0'], testDefaults())

  const distPathExists = fs.existsSync(distPath)
  expect(distPathExists).toBeFalsy()
})

test('refetch package to store if it has been modified', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], testDefaults({ fastUnpack: false }))

  const distPathInStore = project.resolve('magic-hook', '2.0.0', 'dist')
  rimraf(distPathInStore)
  rimraf('node_modules')
  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')

  await addDependenciesToPackage(manifest, ['magic-hook@2.0.0'], testDefaults({ fastUnpack: false }))

  const distPathExists = fs.existsSync(distPath)
  expect(distPathExists).toBeTruthy()
})

// TODO: decide what to do with this case
test.skip('relink package to project if the dependency is not linked from store', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], testDefaults({ save: true, pinnedVersion: 'patch' }))

  const pkgJsonPath = path.resolve('node_modules', 'magic-hook', 'package.json')

  function getInode () {
    return fs.statSync(pkgJsonPath).ino
  }

  const storeInode = getInode()

  // rewriting package.json, to destroy the link
  const pkgJson = fs.readFileSync(pkgJsonPath, 'utf8')
  rimraf(pkgJsonPath)
  fs.writeFileSync(pkgJsonPath, pkgJson, 'utf8')

  expect(storeInode).not.toEqual(getInode())

  await install(manifest, testDefaults({ repeatInstallDepth: 0 }))

  expect(storeInode).toEqual(getInode())
})

test('circular deps', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/circular-deps-1-of-2'], testDefaults({ fastUnpack: false }))

  const m = project.requireModule('@pnpm.e2e/circular-deps-1-of-2/mirror')

  expect(m()).toEqual('@pnpm.e2e/circular-deps-1-of-2')

  expect(fs.existsSync(path.join('node_modules', '@pnpm.e2e/circular-deps-1-of-2', 'node_modules', '@pnpm.e2e/circular-deps-2-of-2', 'node_modules', '@pnpm.e2e/circular-deps-1-of-2'))).toBeFalsy()
})

test('concurrent circular deps', async () => {
  // es5-ext is an external package from the registry
  // the latest dist-tag is overridden to have a stable test
  await addDistTag({ package: 'es5-ext', version: '0.10.31', distTag: 'latest' })
  await addDistTag({ package: 'es6-iterator', version: '2.0.1', distTag: 'latest' })

  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['es6-iterator@2.0.0'], testDefaults({ fastUnpack: false }))

  const m = project.requireModule('es6-iterator')

  expect(m).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/es6-iterator@2.0.0/node_modules/es5-ext'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/es6-iterator@2.0.1/node_modules/es5-ext'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/es5-ext@0.10.31/node_modules/es6-iterator'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/es5-ext@0.10.31/node_modules/es6-symbol'))).toBeTruthy()
})

test('concurrent installation of the same packages', async () => {
  const project = prepareEmpty()

  // the same version of core-js is required by two different dependencies
  // of babek-core
  await addDependenciesToPackage({}, ['babel-core@6.21.0'], testDefaults({ fastUnpack: false }))

  const m = project.requireModule('babel-core')

  expect(m).toBeTruthy()
})

test('big with dependencies and circular deps (babel-preset-2015)', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['babel-preset-es2015@6.3.13'], testDefaults({ fastUnpack: false }))

  const m = project.requireModule('babel-preset-es2015')
  expect(typeof m).toEqual('object')
})

test('compiled modules (ursa@0.9.1)', async () => {
  // TODO: fix this for Node.js v7
  if (!isCI || IS_WINDOWS || semver.satisfies(process.version, '>=7.0.0')) {
    console.log('runs only on CI')
    return
  }

  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['ursa@0.9.1'], testDefaults())

  const m = project.requireModule('ursa')
  expect(typeof m).toEqual('object')
})

test('bin specified in the directories property linked to .bin folder', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-directories-bin'], testDefaults({ fastUnpack: false }))

  project.isExecutable('.bin/pkg-with-directories-bin')
})

test('bin specified in the directories property symlinked to .bin folder when prefer-symlinked-executables is true on POSIX', async () => {
  const project = prepareEmpty()

  const opts = testDefaults({ fastUnpack: false, preferSymlinkedExecutables: true })
  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-directories-bin'], opts)

  project.isExecutable('.bin/pkg-with-directories-bin')

  if (!isWindows()) {
    const link = fs.readlinkSync('node_modules/.bin/pkg-with-directories-bin')
    expect(link).toBeTruthy()
  }
})

testOnNonWindows('building native addons', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['diskusage@1.1.3'], testDefaults({ fastUnpack: false }))

  expect(fs.existsSync('node_modules/diskusage/build')).toBeTruthy()
})

test('should update subdep on second install', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({ save: true }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')

  let lockfile = project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const reporter = sinon.spy()

  await install(manifest, testDefaults({ depth: 1, update: true, reporter }))

  expect(reporter.calledWithMatch({
    added: 1,
    level: 'debug',
    name: 'pnpm:stats',
    prefix: process.cwd(),
  } as StatsLog)).toBeTruthy()

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')

  lockfile = project.readLockfile()

  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  expect(deepRequireCwd(['@pnpm.e2e/pkg-with-1-dep', '@pnpm.e2e/dep-of-pkg-with-1-dep', './package.json']).version).toEqual('100.1.0')
})

test('should not update subdep when depth is smaller than depth of package', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({ save: true }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')

  let lockfile = project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(manifest, testDefaults({ depth: 0, update: true }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')

  lockfile = project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  expect(deepRequireCwd(['@pnpm.e2e/pkg-with-1-dep', '@pnpm.e2e/dep-of-pkg-with-1-dep', './package.json']).version).toEqual('100.0.0')
})

test('should install dependency in second project', async () => {
  const project1 = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({ fastUnpack: false, save: true, store: '../store' }))
  expect(project1.requireModule('@pnpm.e2e/pkg-with-1-dep')().name).toEqual('@pnpm.e2e/dep-of-pkg-with-1-dep')

  const project2 = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({ fastUnpack: false, save: true, store: '../store' }))

  expect(project2.requireModule('@pnpm.e2e/pkg-with-1-dep')().name).toEqual('@pnpm.e2e/dep-of-pkg-with-1-dep')
})

test('should throw error when trying to install using a different store then the previous one', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive'], testDefaults({ storeDir: 'node_modules/.store1' }))

  await expect(
    addDependenciesToPackage(manifest, ['is-negative'], testDefaults({ storeDir: 'node_modules/.store2' }))
  ).rejects.toThrow(new UnexpectedStoreError({
    expectedStorePath: '',
    actualStorePath: '',
    modulesDir: '',
  }))
})

test('ignores drive case in store path', async () => {
  if (!isWindows()) return

  prepareEmpty()

  // paths are case-insensitive on windows, so we will test with an upper and lower-case store
  const storePathUpper: string = path.resolve('node_modules/.store1').toUpperCase()
  const storePathLower: string = storePathUpper.toLowerCase()

  const manifest = await addDependenciesToPackage(
    {},
    ['rimraf@2.5.1'],
    testDefaults({ storeDir: storePathUpper }, null, null, { ignoreFile: () => {} }) // eslint-disable-line:no-empty
  )
  await addDependenciesToPackage(manifest, ['is-negative'], testDefaults({ storeDir: storePathLower }))
})

test('should not throw error if using a different store after all the packages were uninstalled', async () => {
  // TODO: implement
})

test('should throw error when trying to install using a different virtual store directory then the previous one', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive'], testDefaults({ virtualStoreDir: 'pkgs' }))

  await expect(
    addDependenciesToPackage(manifest, ['is-negative'], testDefaults({ virtualStoreDir: 'pnpm' }))
  ).rejects.toThrow(new UnexpectedVirtualStoreDirError({
    actual: '',
    expected: '',
    modulesDir: '',
  }))
})

test('lockfile locks npm dependencies', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({ save: true, reporter }))

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:progress',
    packageId: '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    requester: process.cwd(),
    status: 'resolved',
  } as ProgressLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    requester: process.cwd(),
    status: 'fetched',
  } as ProgressLog)).toBeTruthy()

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  rimraf('node_modules')

  reporter.resetHistory()
  await install(manifest, testDefaults({ reporter }))

  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    requester: process.cwd(),
    status: 'resolved',
  } as ProgressLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    requester: process.cwd(),
    status: 'found_in_store',
  } as ProgressLog)).toBeTruthy()

  const m = project.requireModule('.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json')

  expect(m.version).toEqual('100.0.0')
})

test('self-require should work', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/uses-pkg-with-self-usage'], testDefaults({ fastUnpack: false }))

  expect(project.requireModule('@pnpm.e2e/uses-pkg-with-self-usage')).toBeTruthy()
})

test('install on project with lockfile and no node_modules', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-negative'], testDefaults())

  rimraf('node_modules')

  await addDependenciesToPackage(manifest, ['is-positive'], testDefaults())

  project.has('is-positive') // installed new dependency

  // We have to install all other direct dependencies in case they resolve some peers
  project.has('is-negative')
})

test('install a dependency with * range', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await install({
    dependencies: {
      '@pnpm.e2e/has-beta-only': '*',
    },
  }, testDefaults({ reporter }))

  project.has('@pnpm.e2e/has-beta-only')

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: {
      dependencies: {
        '@pnpm.e2e/has-beta-only': '*',
      },
    } as ProjectManifest,
  } as PackageManifestLog)).toBeTruthy()
})

test('should throw error when trying to install a package without name', async () => {
  prepareEmpty()
  await expect(
    addDependenciesToPackage({}, [`file:${f.find('missing-pkg-name.tgz')}`], testDefaults())
  ).rejects.toThrow(/^Can't install .*: Missing package name$/)
})

// Covers https://github.com/pnpm/pnpm/issues/1193
test('rewrites node_modules created by npm', async () => {
  const project = prepare()

  await execa('npm', ['install', 'rimraf@2.5.1', '@types/node', '--save'])

  const manifest = await install({}, testDefaults())

  const m = project.requireModule('rimraf')
  expect(typeof m).toEqual('function')
  project.isExecutable('.bin/rimraf')

  await execa('npm', ['install', '-f', 'rimraf@2.5.1', '@types/node', '--save'])

  await install(manifest, testDefaults())
})

// Covers https://github.com/pnpm/pnpm/issues/1685
// also, there's a better version of this test (with the same name) in the pnpm package
// TODO: move this test to @pnpm/package-store
test("don't fail on case insensitive filesystems when package has 2 files with same name", async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  const opts = testDefaults({ reporter })
  await addDependenciesToPackage({}, ['@pnpm.e2e/with-same-file-in-different-cases'], opts)

  project.has('@pnpm.e2e/with-same-file-in-different-cases')
})

// Covers https://github.com/pnpm/pnpm/issues/1134
test('reinstalls missing packages to node_modules', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()
  const depLocation = path.resolve('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive')
  const missingDepLog = {
    level: 'debug',
    missing: depLocation,
    name: 'pnpm:_broken_node_modules',
  }

  const opts = testDefaults({ fastUnpack: false, reporter })
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], opts)

  expect(reporter.calledWithMatch(missingDepLog)).toBeFalsy()

  rimraf('pnpm-lock.yaml')
  rimraf('node_modules/is-positive')
  rimraf(depLocation)

  project.hasNot('is-positive')

  reporter.resetHistory()

  await install(manifest, opts)

  expect(reporter.calledWithMatch(missingDepLog)).toBeTruthy()
  project.has('is-positive')
})

// Covers https://github.com/pnpm/pnpm/issues/1134
test('reinstalls missing packages to node_modules during headless install', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()
  const depLocation = path.resolve('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive')
  const missingDepLog = {
    level: 'debug',
    missing: depLocation,
    name: 'pnpm:_broken_node_modules',
  }

  const opts = testDefaults({ fastUnpack: false, reporter })
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], opts)

  expect(reporter.calledWithMatch(missingDepLog)).toBeFalsy()

  rimraf('node_modules/is-positive')
  rimraf(depLocation)

  project.hasNot('is-positive')

  reporter.resetHistory()

  await install(manifest, opts)

  expect(reporter.calledWithMatch(missingDepLog)).toBeTruthy()
  project.has('is-positive')
})

test('do not update deps when lockfile is present', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/peer-a'], testDefaults({ lockfileOnly: true }))

  const initialLockfile = project.readLockfile()

  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.1', distTag: 'latest' })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ preferFrozenLockfile: false }))

  const latestLockfile = project.readLockfile()

  expect(initialLockfile).toStrictEqual(latestLockfile)
})

test('all the subdeps of dependencies are linked when a node_modules is partially up to date', async () => {
  prepareEmpty()

  await mutateModulesInSingleProject({
    manifest: {
      dependencies: {
        '@pnpm.e2e/foobarqar': '1.0.0',
      },
    },
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())

  writeYamlFile(path.resolve('pnpm-lock.yaml'), {
    dependencies: {
      '@pnpm.e2e/foobarqar': {
        specifier: '1.0.1',
        version: '1.0.1',
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '@pnpm.e2e/bar@100.0.0': {
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/bar', '100.0.0'),
        },
      },
      '@pnpm.e2e/foo@100.1.0': {
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/foo', '100.1.0'),
        },
      },
      '@pnpm.e2e/foobarqar@1.0.1': {
        dependencies: {
          '@pnpm.e2e/bar': '100.0.0',
          '@pnpm.e2e/foo': '100.1.0',
          'is-positive': '3.1.0',
        },
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/foobarqar', '1.0.1'),
        },
      },
      'is-positive@3.1.0': {
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
        },
      },
    },
  }, { lineWidth: 1000 })

  await mutateModulesInSingleProject({
    manifest: {
      dependencies: {
        '@pnpm.e2e/foobarqar': '1.0.1',
      },
    },
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ preferFrozenLockfile: false }))

  expect(
    [...fs.readdirSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+foobarqar@1.0.1/node_modules/@pnpm.e2e'))].sort()
  ).toStrictEqual(
    [
      'bar',
      'foo',
      'foobarqar',
      'qar',
    ].sort()
  )
})

test('subdep symlinks are updated if the lockfile has new subdep versions specified', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })
  const project = prepareEmpty()

  await mutateModulesInSingleProject({
    manifest: {
      dependencies: {
        '@pnpm.e2e/parent-of-pkg-with-1-dep': '1.0.0',
      },
    },
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())

  const lockfile = project.readLockfile()

  expect(
    Object.keys(lockfile.packages)
  ).toStrictEqual(
    [
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0',
      '@pnpm.e2e/parent-of-pkg-with-1-dep@1.0.0',
      '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    ]
  )

  writeYamlFile(path.resolve('pnpm-lock.yaml'), {
    dependencies: {
      '@pnpm.e2e/parent-of-pkg-with-1-dep': {
        specifier: '1.0.0',
        version: '1.0.0',
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0'),
        },
      },
      '@pnpm.e2e/parent-of-pkg-with-1-dep@1.0.0': {
        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/parent-of-pkg-with-1-dep', '1.0.0'),
        },
      },
      '@pnpm.e2e/pkg-with-1-dep@100.0.0': {
        dependencies: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.1.0',
        },
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/pkg-with-1-dep', '100.0.0'),
        },
      },
    },
  }, { lineWidth: 1000 })

  await mutateModulesInSingleProject({
    manifest: {
      dependencies: {
        '@pnpm.e2e/parent-of-pkg-with-1-dep': '1.0.0',
      },
    },
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ preferFrozenLockfile: false }))

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
})

test('globally installed package which don\'t have bins should log warning message', async () => {
  prepareEmpty()
  const reporter = sinon.spy()

  const opts = testDefaults({ global: true, reporter })

  await mutateModulesInSingleProject({
    manifest: {
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, opts)

  expect(reporter.calledWithMatch({
    message: 'is-positive has no binaries',
    prefix: process.cwd(),
  })).toBeTruthy()
})

// Covers issue: https://github.com/pnpm/pnpm/issues/2629
test('installing a package that has a manifest with byte order mark (BOM)', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      paralleljs: '0.2.1',
    },
  }, testDefaults())

  project.has('paralleljs')
})

test('ignore files in node_modules', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  fs.mkdirSync('node_modules')
  fs.writeFileSync('node_modules/foo', 'x', 'utf8')

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['lodash@4.0.0'],
    testDefaults({ fastUnpack: false, reporter })
  )

  const m = project.requireModule('lodash')
  expect(typeof m).toEqual('function')
  expect(typeof m.clone).toEqual('function')
  expect(fs.readFileSync('node_modules/foo', 'utf8')).toEqual('x')
})

// Covers https://github.com/pnpm/pnpm/issues/2339
test('memory consumption is under control on huge package with many peer dependencies. Sample 1', async () => {
  prepareEmpty()

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['@teambit/bit@0.0.30'],
    testDefaults({ fastUnpack: true, lockfileOnly: true, strictPeerDependencies: false })
  )

  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()
})

// Covers https://github.com/pnpm/pnpm/issues/2339
test('memory consumption is under control on huge package with many peer dependencies. Sample 2', async () => {
  prepareEmpty()

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['@teambit/react@0.0.30'],
    testDefaults({ fastUnpack: true, lockfileOnly: true, strictPeerDependencies: false })
  )

  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()
})

test('installing with no symlinks with PnP', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['rimraf@2.7.1'],
    testDefaults({
      enablePnp: true,
      fastUnpack: false,
      symlink: false,
    })
  )

  expect([...fs.readdirSync(path.resolve('node_modules'))]).toStrictEqual(['.bin', '.modules.yaml', '.pnpm'])
  expect([...fs.readdirSync(path.resolve('node_modules/.pnpm/rimraf@2.7.1/node_modules'))]).toStrictEqual(['rimraf'])

  expect(project.readCurrentLockfile()).toBeTruthy()
  expect(project.readModulesManifest()).toBeTruthy()
  expect(fs.existsSync(path.resolve('.pnp.cjs'))).toBeTruthy()
})

test('installing with no modules directory', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['rimraf@2.7.1'],
    testDefaults({
      enableModulesDir: false,
      fastUnpack: false,
    })
  )

  expect(project.readLockfile()).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules'))).toBeFalsy()
})

test('installing dependencies with the same name in different case', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
  ])

  await mutateModulesInSingleProject({
    mutation: 'install',
    manifest: {
      dependencies: {
        File: 'https://registry.npmjs.org/File/-/File-0.10.2.tgz',
        file: 'https://registry.npmjs.org/file/-/file-0.2.2.tgz',
      },
    },
    rootDir: path.resolve('project-1') as ProjectRootDir,
  }, testDefaults({ fastUnpack: false }))

  // if it did not fail, it is fine
})

test('two dependencies have the same version and name. The only difference is the casing in the name', async () => {
  prepareEmpty()

  await mutateModulesInSingleProject({
    mutation: 'install',
    manifest: {
      dependencies: {
        a: 'npm:JSONStream@1.0.3',
        b: 'npm:jsonstream@1.0.3',
      },
    },
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({
    fastUnpack: false,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  }))

  expect(fs.readdirSync(path.resolve('node_modules/.pnpm')).length).toBe(5)
})

test('installing a package with broken bin', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/broken-bin@1.0.0'], testDefaults({ fastUnpack: false }))

  project.has('@pnpm.e2e/broken-bin')
})

test('a package should be able to be a dependency of itself', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@paul-soporan/test-package-self-require-trap@2.0.0'], testDefaults())

  const subpkg = '.pnpm/@paul-soporan+test-package-self-require-trap@2.0.0/node_modules/@paul-soporan/test-package-self-require-trap/node_modules/@paul-soporan/test-package-self-require-trap/package.json'
  {
    const pkg = project.requireModule(subpkg)
    expect(pkg.version).toBe('1.0.0')
  }

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true }))

  {
    const pkg = project.requireModule(subpkg)
    expect(pkg.version).toBe('1.0.0')
  }
})
