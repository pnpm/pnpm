import * as path from 'path'
import { promises as fs } from 'fs'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import PnpmError from '@pnpm/error'
import {
  PackageManifestLog,
  ProgressLog,
  RootLog,
  StageLog,
  StatsLog,
} from '@pnpm/core-loggers'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import fixtures from '@pnpm/test-fixtures'
import { ProjectManifest } from '@pnpm/types'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
  UnexpectedStoreError,
  UnexpectedVirtualStoreDirError,
} from '@pnpm/core'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import isCI from 'is-ci'
import isWindows from 'is-windows'
import exists from 'path-exists'
import semver from 'semver'
import sinon from 'sinon'
import deepRequireCwd from 'deep-require-cwd'
import writeYamlFile from 'write-yaml-file'
import {
  addDistTag,
  testDefaults,
} from '../utils'

const f = fixtures(__dirname)
const IS_WINDOWS = isWindows()

const testOnNonWindows = IS_WINDOWS ? test.skip : test

test('spec not specified in package.json.dependencies', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      'is-positive': '',
    },
  }, await testDefaults())

  const lockfile = await project.readLockfile()
  expect(lockfile.specifiers['is-positive']).toBe('')
})

test('ignoring some files in the dependency', async () => {
  prepareEmpty()

  const ignoreFile = (filename: string) => filename === 'readme.md'
  await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults({}, {}, { ignoreFile }))

  // package.json was not ignored
  expect(await exists(path.resolve('node_modules', 'is-positive', 'package.json'))).toBeTruthy()
  // readme.md was ignored
  expect(await exists(path.resolve('node_modules', 'is-positive', 'readme.md'))).toBeFalsy()
})

test('no dependencies (lodash)', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await addDistTag('lodash', '4.1.0', 'latest')

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['lodash@4.0.0'],
    await testDefaults({ fastUnpack: false, reporter })
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

test('scoped modules without version spec', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@zkochan/foo'], await testDefaults())

  await project.has('@zkochan/foo')
})

test('scoped package with custom registry', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@scoped/peer'], await testDefaults({
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

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', tag)

  await addDependenciesToPackage({}, ['dep-of-pkg-with-1-dep'], await testDefaults({ tag }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('modules without version spec but with a trailing @', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['dep-of-pkg-with-1-dep@'], await testDefaults())

  await project.has('dep-of-pkg-with-1-dep')
})

test('aliased modules without version spec but with a trailing @', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['foo@npm:dep-of-pkg-with-1-dep@'], await testDefaults())

  await project.has('foo')
})

test('installing a package by specifying a specific dist-tag', async () => {
  const project = prepareEmpty()

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'beta')

  await addDependenciesToPackage({}, ['dep-of-pkg-with-1-dep@beta'], await testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('update a package when installing with a dist-tag', async () => {
  const project = prepareEmpty()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'beta')

  const manifest = await addDependenciesToPackage({}, ['dep-of-pkg-with-1-dep'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  const reporter = sinon.spy()

  await addDependenciesToPackage(manifest, ['dep-of-pkg-with-1-dep@beta'], await testDefaults({ targetDependenciesField: 'devDependencies', reporter }))

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      dependencyType: 'dev',
      name: 'dep-of-pkg-with-1-dep',
      version: '100.0.0',
    },
  } as RootLog)).toBeTruthy()

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: 'dep-of-pkg-with-1-dep',
      version: '100.1.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()

  await project.has('dep-of-pkg-with-1-dep')
  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')

  expect(manifest.devDependencies!['dep-of-pkg-with-1-dep']).toBe('^100.1.0')
})

test('scoped modules with versions', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@zkochan/foo@1.0.0'], await testDefaults({ fastUnpack: false }))

  await project.has('@zkochan/foo')
})

test('multiple scoped modules (@rstacruz/...)', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], await testDefaults({ fastUnpack: false }))

  expect(typeof project.requireModule('@rstacruz/tap-spec')).toBe('function')
  expect(typeof project.requireModule('@rstacruz/travis-encrypt')).toBe('function')
})

test('nested scoped modules (test-pnpm-issue219 -> @zkochan/test-pnpm-issue219)', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['test-pnpm-issue219@1.0.2'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('test-pnpm-issue219')
  expect(m).toBe('test-pnpm-issue219,@zkochan/test-pnpm-issue219')
})

test('idempotency', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()
  const opts = await testDefaults({ reporter })

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep@100.0.0'], opts)

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'pkg-with-1-dep',
      version: '100.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()

  reporter.resetHistory()

  await addDependenciesToPackage(manifest, ['pkg-with-1-dep@100.0.0'], opts)

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'pkg-with-1-dep',
      version: '100.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeFalsy()

  await project.has('pkg-with-1-dep')
})

test('reporting adding root package', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults())

  await project.storeHas('flatten', '1.0.2')

  const reporter = sinon.spy()

  await addDependenciesToPackage(manifest, ['flatten@1.0.2'], await testDefaults({ reporter }))

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
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults())

  await project.storeHas('flatten', '1.0.2')

  await addDependenciesToPackage(manifest, ['magic-hook@0.1.0'], await testDefaults())

  // flatten is not removed from store even though it is unreferenced
  // store should be pruned to have this removed
  await project.storeHas('flatten', '1.0.2')

  const m = project.requireModule('magic-hook/package.json')
  expect(m.version).toBe('0.1.0')
})

test('overwriting (is-positive@3.0.0 with is-positive@latest)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['is-positive@3.0.0'], await testDefaults({ save: true }))

  await project.storeHas('is-positive', '3.0.0')

  const updatedManifest = await addDependenciesToPackage(manifest, ['is-positive@latest'], await testDefaults({ save: true }))

  await project.storeHas('is-positive', '3.1.0')
  expect(updatedManifest.dependencies?.['is-positive']).toBe('3.1.0')
})

// Covers https://github.com/pnpm/pnpm/issues/2188
test('keeping existing specs untouched when adding new dependency', async () => {
  prepareEmpty()

  await addDistTag('bar', '100.1.0', 'latest')

  const manifest = await addDependenciesToPackage({ dependencies: { bar: '^100.0.0' } }, ['foo@100.1.0'], await testDefaults())

  expect(manifest.dependencies).toStrictEqual({ bar: '^100.0.0', foo: '100.1.0' })
})

test('forcing', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false }))

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  await rimraf(distPath)

  await addDependenciesToPackage(manifest, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false, force: true }))

  const distPathExists = await exists(distPath)
  expect(distPathExists).toBeTruthy()
})

test('argumentless forcing', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false }))

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  await rimraf(distPath)

  await install(manifest, await testDefaults({ fastUnpack: false, force: true }))

  const distPathExists = await exists(distPath)
  expect(distPathExists).toBeTruthy()
})

test('no forcing', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults())

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  await rimraf(distPath)

  await addDependenciesToPackage(manifest, ['magic-hook@2.0.0'], await testDefaults())

  const distPathExists = await exists(distPath)
  expect(distPathExists).toBeFalsy()
})

test('refetch package to store if it has been modified', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false }))

  const distPathInStore = await project.resolve('magic-hook', '2.0.0', 'dist')
  await rimraf(distPathInStore)
  await rimraf('node_modules')
  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')

  await addDependenciesToPackage(manifest, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false }))

  const distPathExists = await exists(distPath)
  expect(distPathExists).toBeTruthy()
})

// TODO: decide what to do with this case
// eslint-disable-next-line @typescript-eslint/dot-notation
test.skip('relink package to project if the dependency is not linked from store', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults({ save: true, pinnedVersion: 'patch' }))

  const pkgJsonPath = path.resolve('node_modules', 'magic-hook', 'package.json')

  async function getInode () {
    return (await fs.stat(pkgJsonPath)).ino
  }

  const storeInode = await getInode()

  // rewriting package.json, to destroy the link
  const pkgJson = await fs.readFile(pkgJsonPath, 'utf8')
  await rimraf(pkgJsonPath)
  await fs.writeFile(pkgJsonPath, pkgJson, 'utf8')

  expect(storeInode).not.toEqual(await getInode())

  await install(manifest, await testDefaults({ repeatInstallDepth: 0 }))

  expect(storeInode).toEqual(await getInode())
})

test('circular deps', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['circular-deps-1-of-2'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('circular-deps-1-of-2/mirror')

  expect(m()).toEqual('circular-deps-1-of-2')

  expect(await exists(path.join('node_modules', 'circular-deps-1-of-2', 'node_modules', 'circular-deps-2-of-2', 'node_modules', 'circular-deps-1-of-2'))).toBeFalsy()
})

test('concurrent circular deps', async () => {
  // es5-ext is an external package from the registry
  // the latest dist-tag is overriden to have a stable test
  await addDistTag('es5-ext', '0.10.31', 'latest')
  await addDistTag('es6-iterator', '2.0.1', 'latest')

  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['es6-iterator@2.0.0'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('es6-iterator')

  expect(m).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/es6-iterator@2.0.0/node_modules/es5-ext'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/es6-iterator@2.0.1/node_modules/es5-ext'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/es5-ext@0.10.31/node_modules/es6-iterator'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/es5-ext@0.10.31/node_modules/es6-symbol'))).toBeTruthy()
})

test('concurrent installation of the same packages', async () => {
  const project = prepareEmpty()

  // the same version of core-js is required by two different dependencies
  // of babek-core
  await addDependenciesToPackage({}, ['babel-core@6.21.0'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('babel-core')

  expect(m).toBeTruthy()
})

test('big with dependencies and circular deps (babel-preset-2015)', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['babel-preset-es2015@6.3.13'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('babel-preset-es2015')
  expect(typeof m).toEqual('object')
})

test('bundledDependencies (pkg-with-bundled-dependencies@1.0.0)', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['pkg-with-bundled-dependencies@1.0.0'], await testDefaults({ fastUnpack: false }))

  await project.isExecutable('pkg-with-bundled-dependencies/node_modules/.bin/hello-world-js-bin')

  const lockfile = await project.readLockfile()
  expect(
    lockfile.packages['/pkg-with-bundled-dependencies/1.0.0'].bundledDependencies
  ).toStrictEqual(
    ['hello-world-js-bin']
  )
})

test('bundleDependencies (pkg-with-bundle-dependencies@1.0.0)', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['pkg-with-bundle-dependencies@1.0.0'], await testDefaults({ fastUnpack: false }))

  await project.isExecutable('pkg-with-bundle-dependencies/node_modules/.bin/hello-world-js-bin')

  const lockfile = await project.readLockfile()
  expect(
    lockfile.packages['/pkg-with-bundle-dependencies/1.0.0'].bundledDependencies
  ).toStrictEqual(
    ['hello-world-js-bin']
  )
})

test('installing a package with bundleDependencies set to false (pkg-with-bundle-dependencies-false)', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['pkg-with-bundle-dependencies-false'], await testDefaults({ fastUnpack: false }))

  const lockfile = await project.readLockfile()
  expect(
    typeof lockfile.packages['/pkg-with-bundle-dependencies-false/1.0.0'].bundledDependencies
  ).toEqual('undefined')
})

test('compiled modules (ursa@0.9.1)', async () => {
  // TODO: fix this for Node.js v7
  if (!isCI || IS_WINDOWS || semver.satisfies(process.version, '>=7.0.0')) {
    console.log('runs only on CI')
    return
  }

  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['ursa@0.9.1'], await testDefaults())

  const m = project.requireModule('ursa')
  expect(typeof m).toEqual('object')
})

test('bin specified in the directories property linked to .bin folder', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['pkg-with-directories-bin'], await testDefaults({ fastUnpack: false }))

  await project.isExecutable('.bin/pkg-with-directories-bin')
})

testOnNonWindows('building native addons', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['diskusage@1.1.3'], await testDefaults({ fastUnpack: false }))

  expect(await exists('node_modules/diskusage/build')).toBeTruthy()

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/diskusage/1.1.3', 'requiresBuild'], true)
})

test('should update subdep on second install', async () => {
  const project = prepareEmpty()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ save: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  let lockfile = await project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/100.0.0'])

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  const reporter = sinon.spy()

  await install(manifest, await testDefaults({ depth: 1, update: true, reporter }))

  expect(reporter.calledWithMatch({
    added: 1,
    level: 'debug',
    name: 'pnpm:stats',
    prefix: process.cwd(),
  } as StatsLog)).toBeTruthy()

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')

  lockfile = await project.readLockfile()

  expect(lockfile.packages).not.toHaveProperty(['/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/100.1.0'])

  expect(deepRequireCwd(['pkg-with-1-dep', 'dep-of-pkg-with-1-dep', './package.json']).version).toEqual('100.1.0')
})

test('should not update subdep when depth is smaller than depth of package', async () => {
  const project = prepareEmpty()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ save: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  let lockfile = await project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/100.0.0'])

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(manifest, await testDefaults({ depth: 0, update: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  lockfile = await project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/dep-of-pkg-with-1-dep/100.1.0'])

  expect(deepRequireCwd(['pkg-with-1-dep', 'dep-of-pkg-with-1-dep', './package.json']).version).toEqual('100.0.0')
})

test('should install dependency in second project', async () => {
  const project1 = prepareEmpty()

  await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ fastUnpack: false, save: true, store: '../store' }))
  expect(project1.requireModule('pkg-with-1-dep')().name).toEqual('dep-of-pkg-with-1-dep')

  const project2 = prepareEmpty()

  await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ fastUnpack: false, save: true, store: '../store' }))

  expect(project2.requireModule('pkg-with-1-dep')().name).toEqual('dep-of-pkg-with-1-dep')
})

test('should throw error when trying to install using a different store then the previous one', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ storeDir: 'node_modules/.store1' }))

  await expect(
    addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ storeDir: 'node_modules/.store2' }))
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
    await testDefaults({ storeDir: storePathUpper }, null, null, { ignoreFile: () => {} }) // eslint-disable-line:no-empty
  )
  await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ storeDir: storePathLower }))
})

test('should not throw error if using a different store after all the packages were uninstalled', async () => {
  // TODO: implement
})

test('should throw error when trying to install using a different virtual store directory then the previous one', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ virtualStoreDir: 'pkgs' }))

  await expect(
    addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ virtualStoreDir: 'pnpm' }))
  ).rejects.toThrow(new UnexpectedVirtualStoreDirError({
    actual: '',
    expected: '',
    modulesDir: '',
  }))
})

test('lockfile locks npm dependencies', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ save: true, reporter }))

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:progress',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep/100.0.0`,
    requester: process.cwd(),
    status: 'resolved',
  } as ProgressLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep/100.0.0`,
    requester: process.cwd(),
    status: 'fetched',
  } as ProgressLog)).toBeTruthy()

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await rimraf('node_modules')

  reporter.resetHistory()
  await install(manifest, await testDefaults({ reporter }))

  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep/100.0.0`,
    requester: process.cwd(),
    status: 'resolved',
  } as ProgressLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep/100.0.0`,
    requester: process.cwd(),
    status: 'found_in_store',
  } as ProgressLog)).toBeTruthy()

  const m = project.requireModule('.pnpm/pkg-with-1-dep@100.0.0/node_modules/dep-of-pkg-with-1-dep/package.json')

  expect(m.version).toEqual('100.0.0')
})
test('self-require should work', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['uses-pkg-with-self-usage'], await testDefaults({ fastUnpack: false }))

  expect(project.requireModule('uses-pkg-with-self-usage')).toBeTruthy()
})

test('install on project with lockfile and no node_modules', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-negative'], await testDefaults())

  await rimraf('node_modules')

  await addDependenciesToPackage(manifest, ['is-positive'], await testDefaults())

  await project.has('is-positive') // installed new dependency

  // We have to install all other direct dependencies in case they resolve some peers
  await project.has('is-negative')
})

test('install a dependency with * range', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await install({
    dependencies: {
      'has-beta-only': '*',
    },
  }, await testDefaults({ reporter }))

  await project.has('has-beta-only')

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: {
      dependencies: {
        'has-beta-only': '*',
      },
    } as ProjectManifest,
  } as PackageManifestLog)).toBeTruthy()
})

test('should throw error when trying to install a package without name', async () => {
  prepareEmpty()
  await expect(
    addDependenciesToPackage({}, [`file:${f.find('missing-pkg-name.tgz')}`], await testDefaults())
  ).rejects.toThrow(/^Can't install .*: Missing package name$/)
})

// Covers https://github.com/pnpm/pnpm/issues/1193
test('rewrites node_modules created by npm', async () => {
  const project = prepareEmpty()

  await execa('npm', ['install', 'rimraf@2.5.1', '@types/node', '--save'])

  const manifest = await install({}, await testDefaults())

  const m = project.requireModule('rimraf')
  expect(typeof m).toEqual('function')
  await project.isExecutable('.bin/rimraf')

  await execa('npm', ['install', '-f', 'rimraf@2.5.1', '@types/node', '--save'])

  await install(manifest, await testDefaults())
})

// Covers https://github.com/pnpm/pnpm/issues/1685
// also, there's a better version of this test (with the same name) in the pnpm package
// TODO: move this test to @pnpm/package-store
test("don't fail on case insensitive filesystems when package has 2 files with same name", async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  const opts = await testDefaults({ reporter })
  await addDependenciesToPackage({}, ['with-same-file-in-different-cases'], opts)

  await project.has('with-same-file-in-different-cases')
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

  const opts = await testDefaults({ fastUnpack: false, reporter })
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], opts)

  expect(reporter.calledWithMatch(missingDepLog)).toBeFalsy()

  await rimraf('pnpm-lock.yaml')
  await rimraf('node_modules/is-positive')
  await rimraf(depLocation)

  await project.hasNot('is-positive')

  reporter.resetHistory()

  await install(manifest, opts)

  expect(reporter.calledWithMatch(missingDepLog)).toBeTruthy()
  await project.has('is-positive')
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

  const opts = await testDefaults({ fastUnpack: false, reporter })
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], opts)

  expect(reporter.calledWithMatch(missingDepLog)).toBeFalsy()

  await rimraf('node_modules/is-positive')
  await rimraf(depLocation)

  await project.hasNot('is-positive')

  reporter.resetHistory()

  await install(manifest, opts)

  expect(reporter.calledWithMatch(missingDepLog)).toBeTruthy()
  await project.has('is-positive')
})

test('do not update deps when lockfile is present', async () => {
  await addDistTag('peer-a', '1.0.0', 'latest')
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['peer-a'], await testDefaults({ lockfileOnly: true }))

  const initialLockfile = await project.readLockfile()

  await addDistTag('peer-a', '1.0.1', 'latest')

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ preferFrozenLockfile: false }))

  const latestLockfile = await project.readLockfile()

  expect(initialLockfile).toStrictEqual(latestLockfile)
})

test('all the subdeps of dependencies are linked when a node_modules is partially up-to-date', async () => {
  prepareEmpty()

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          foobarqar: '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  await writeYamlFile(path.resolve('pnpm-lock.yaml'), {
    dependencies: {
      foobarqar: '1.0.1',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/bar/100.0.0': {
        dev: false,
        resolution: {
          integrity: getIntegrity('bar', '100.0.0'),
        },
      },
      '/foo/100.1.0': {
        dev: false,
        resolution: {
          integrity: getIntegrity('foo', '100.1.0'),
        },
      },
      '/foobarqar/1.0.1': {
        dependencies: {
          bar: '100.0.0',
          foo: '100.1.0',
          'is-positive': '3.1.0',
        },
        dev: false,
        resolution: {
          integrity: getIntegrity('foobarqar', '1.0.1'),
        },
      },
      '/is-positive/3.1.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
        },
      },
    },
    specifiers: {
      foobarqar: '1.0.1',
    },
  }, { lineWidth: 1000 })

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          foobarqar: '1.0.1',
        },
      },
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ preferFrozenLockfile: false }))

  expect(
    [...await fs.readdir(path.resolve('node_modules/.pnpm/foobarqar@1.0.1/node_modules'))]
  ).toStrictEqual(
    [
      'bar',
      'foo',
      'foobarqar',
      'qar',
    ]
  )
})

test('subdep symlinks are updated if the lockfile has new subdep versions specified', async () => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')
  const project = prepareEmpty()

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'parent-of-pkg-with-1-dep': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  const lockfile = await project.readLockfile()

  expect(
    Object.keys(lockfile.packages)
  ).toStrictEqual(
    [
      '/dep-of-pkg-with-1-dep/100.0.0',
      '/parent-of-pkg-with-1-dep/1.0.0',
      '/pkg-with-1-dep/100.0.0',
    ]
  )

  await writeYamlFile(path.resolve('pnpm-lock.yaml'), {
    dependencies: {
      'parent-of-pkg-with-1-dep': '1.0.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/dep-of-pkg-with-1-dep/100.1.0': {
        dev: false,
        resolution: {
          integrity: getIntegrity('dep-of-pkg-with-1-dep', '100.1.0'),
        },
      },
      '/parent-of-pkg-with-1-dep/1.0.0': {
        dependencies: {
          'pkg-with-1-dep': '100.0.0',
        },
        dev: false,
        resolution: {
          integrity: getIntegrity('parent-of-pkg-with-1-dep', '1.0.0'),
        },
      },
      '/pkg-with-1-dep/100.0.0': {
        dependencies: {
          'dep-of-pkg-with-1-dep': '100.1.0',
        },
        dev: false,
        resolution: {
          integrity: getIntegrity('pkg-with-1-dep', '100.0.0'),
        },
      },
    },
    specifiers: {
      'parent-of-pkg-with-1-dep': '1.0.0',
    },
  }, { lineWidth: 1000 })

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'parent-of-pkg-with-1-dep': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ preferFrozenLockfile: false }))

  expect(await exists(path.resolve('node_modules/.pnpm/pkg-with-1-dep@100.0.0/node_modules/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
})

test('fail if none of the available resolvers support a version spec', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await mutateModules([
      {
        buildIndex: 0,
        manifest: {
          dependencies: {
            '@types/plotly.js': '1.44.29',
          },
        },
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ], await testDefaults())
    throw new Error('should have failed')
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER')
  expect(err.pkgsStack).toStrictEqual(
    [
      {
        id: `localhost+${REGISTRY_MOCK_PORT}/@types/plotly.js/1.44.29`,
        name: '@types/plotly.js',
        version: '1.44.29',
      },
    ]
  )
})

test('globally installed package which don\'t have bins should log warning message', async () => {
  prepareEmpty()
  const reporter = sinon.spy()

  const opts = await testDefaults({ global: true, reporter })

  await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], opts)

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
  }, await testDefaults())

  await project.has('paralleljs')
})

test('ignore files in node_modules', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await fs.mkdir('node_modules')
  await fs.writeFile('node_modules/foo', 'x', 'utf8')

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['lodash@4.0.0'],
    await testDefaults({ fastUnpack: false, reporter })
  )

  const m = project.requireModule('lodash')
  expect(typeof m).toEqual('function')
  expect(typeof m.clone).toEqual('function')
  expect(await fs.readFile('node_modules/foo', 'utf8')).toEqual('x')
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
    await testDefaults({ fastUnpack: true, lockfileOnly: true })
  )

  expect(await exists('pnpm-lock.yaml')).toBeTruthy()
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
    await testDefaults({ fastUnpack: true, lockfileOnly: true })
  )

  expect(await exists('pnpm-lock.yaml')).toBeTruthy()
})

test('installing with no symlinks with PnP', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['rimraf@2.7.1'],
    await testDefaults({
      enablePnp: true,
      fastUnpack: false,
      symlink: false,
    })
  )

  expect([...await fs.readdir(path.resolve('node_modules'))]).toStrictEqual(['.bin', '.modules.yaml', '.pnpm'])
  expect([...await fs.readdir(path.resolve('node_modules/.pnpm/rimraf@2.7.1/node_modules'))]).toStrictEqual(['rimraf'])

  expect(await project.readCurrentLockfile()).toBeTruthy()
  expect(await project.readModulesManifest()).toBeTruthy()
  expect(await exists(path.resolve('.pnp.cjs'))).toBeTruthy()
})

test('installing with no modules directory', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['rimraf@2.7.1'],
    await testDefaults({
      enableModulesDir: false,
      fastUnpack: false,
    })
  )

  expect(await project.readLockfile()).toBeTruthy()
  expect(await exists(path.resolve('node_modules'))).toBeFalsy()
})

test('installing dependencies with the same name in different case', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
  ])

  await mutateModules([
    {
      buildIndex: 0,
      mutation: 'install',
      manifest: {
        dependencies: {
          File: 'https://registry.npmjs.org/File/-/File-0.10.2.tgz',
          file: 'https://registry.npmjs.org/file/-/file-0.2.2.tgz',
        },
      },
      rootDir: path.resolve('project-1'),
    },
  ], await testDefaults({ fastUnpack: false }))

  // if it did not fail, it is fine
})

test('two dependencies have the same version and name. The only difference is the casing in the name', async () => {
  prepareEmpty()

  await mutateModules([
    {
      buildIndex: 0,
      mutation: 'install',
      manifest: {
        dependencies: {
          a: 'npm:JSONStream@1.0.3',
          b: 'npm:jsonstream@1.0.3',
        },
      },
      rootDir: process.cwd(),
    },
  ], await testDefaults({
    fastUnpack: false,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  }))

  expect((await fs.readdir(path.resolve('node_modules/.pnpm'))).length).toBe(5)
})

test('installing a package with broken bin', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['broken-bin@1.0.0'], await testDefaults({ fastUnpack: false }))

  await project.has('broken-bin')
})

test('a package should be able to be a dependency of itself', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@paul-soporan/test-package-self-require-trap@2.0.0'], await testDefaults())

  const subpkg = '.pnpm/@paul-soporan+test-package-self-require-trap@2.0.0/node_modules/@paul-soporan/test-package-self-require-trap/node_modules/@paul-soporan/test-package-self-require-trap/package.json'
  {
    const pkg = project.requireModule(subpkg)
    expect(pkg.version).toBe('1.0.0')
  }

  await rimraf('node_modules')
  await install(manifest, await testDefaults({ frozenLockfile: true }))

  {
    const pkg = project.requireModule(subpkg)
    expect(pkg.version).toBe('1.0.0')
  }
})
