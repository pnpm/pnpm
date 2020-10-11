import * as path from 'path'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { prepareEmpty } from '@pnpm/prepare'
import UnexpectedVirtualStoreDirError from '@pnpm/get-context/lib/checkCompatibility/UnexpectedVirtualStoreDirError'
import PnpmError from '@pnpm/error'
import {
  PackageManifestLog,
  ProgressLog,
  RootLog,
  StageLog,
  StatsLog,
} from '@pnpm/core-loggers'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { pathToLocalPkg } from '@pnpm/test-fixtures'
import { ProjectManifest } from '@pnpm/types'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
} from 'supi'
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  testDefaults,
} from '../utils'
import rimraf = require('@zkochan/rimraf')
import deepRequireCwd = require('deep-require-cwd')
import execa = require('execa')
import isCI = require('is-ci')
import isWindows = require('is-windows')
import fs = require('mz/fs')
import exists = require('path-exists')
import semver = require('semver')
import sinon = require('sinon')
import tape = require('tape')
import writeYamlFile = require('write-yaml-file')

const test = promisifyTape(tape)

const IS_WINDOWS = isWindows()

test('spec not specified in package.json.dependencies', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'is-positive': '',
    },
  }, await testDefaults())

  const lockfile = await project.readLockfile()
  t.ok(lockfile.specifiers['is-positive'] === '', `spec saved properly in ${WANTED_LOCKFILE}`)
})

test('ignoring some files in the dependency', async (t: tape.Test) => {
  prepareEmpty(t)

  const ignoreFile = (filename: string) => filename === 'readme.md'
  await addDependenciesToPackage({}, ['is-positive@1.0.0'], await testDefaults({}, {}, { ignoreFile }))

  t.ok(await exists(path.resolve('node_modules', 'is-positive', 'package.json')), 'package.json was not ignored')
  t.notOk(await exists(path.resolve('node_modules', 'is-positive', 'readme.md')), 'readme.md was ignored')
})

test('no dependencies (lodash)', async (t: tape.Test) => {
  const project = prepareEmpty(t)
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

  t.equal(reporter.withArgs(sinon.match({
    initial: { name: 'project', version: '0.0.0' },
    level: 'debug',
    name: 'pnpm:package-manifest',
  } as PackageManifestLog)).callCount, 1, 'initial package.json logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix: process.cwd(),
    stage: 'resolution_started',
  } as StageLog), 'resolution stage start logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix: process.cwd(),
    stage: 'resolution_done',
  } as StageLog), 'resolution stage done logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix: process.cwd(),
    stage: 'importing_started',
  } as StageLog), 'importing stage start logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stage',
    prefix: process.cwd(),
    stage: 'importing_done',
  } as StageLog), 'importing stage done logged')
  // Not logged for now
  // t.ok(reporter.calledWithMatch({
  //   level: 'info',
  //   message: 'Creating dependency graph',
  // }), 'informed about creating dependency graph')
  t.ok(reporter.calledWithMatch({
    added: 1,
    level: 'debug',
    name: 'pnpm:stats',
    prefix: process.cwd(),
  } as StatsLog), 'added stat')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix: process.cwd(),
    removed: 0,
  } as StatsLog), 'removed stat')
  t.ok(reporter.calledWithMatch({
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
  } as RootLog), 'added to root')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: {
      dependencies: {
        lodash: '4.0.0',
      },
      name: 'project',
      version: '0.0.0',
    } as ProjectManifest,
  } as PackageManifestLog), 'updated package.json logged')

  const m = project.requireModule('lodash')
  t.ok(typeof m === 'function', '_ is available')
  t.ok(typeof m.clone === 'function', '_.clone is available')
})

test('scoped modules without version spec', async (t) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['@zkochan/foo'], await testDefaults())

  await project.has('@zkochan/foo')
})

test('scoped package with custom registry', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['@scoped/peer'], await testDefaults({
    // setting an incorrect default registry URL
    rawConfig: {
      '@scoped:registry': `http://localhost:${REGISTRY_MOCK_PORT}/`,
    },
    registry: 'http://localhost:9999/',
  }))

  const m = project.requireModule('@scoped/peer/package.json')
  t.ok(m, 'is available')
})

test('modules without version spec, with custom tag config', async (t) => {
  const project = prepareEmpty(t)

  const tag = 'beta'

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', tag)

  await addDependenciesToPackage({}, ['dep-of-pkg-with-1-dep'], await testDefaults({ tag }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('modules without version spec but with a trailing @', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['dep-of-pkg-with-1-dep@'], await testDefaults())

  await project.has('dep-of-pkg-with-1-dep')
})

test('aliased modules without version spec but with a trailing @', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['foo@npm:dep-of-pkg-with-1-dep@'], await testDefaults())

  await project.has('foo')
})

test('installing a package by specifying a specific dist-tag', async (t) => {
  const project = prepareEmpty(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'beta')

  await addDependenciesToPackage({}, ['dep-of-pkg-with-1-dep@beta'], await testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('update a package when installing with a dist-tag', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'beta')

  const manifest = await addDependenciesToPackage({}, ['dep-of-pkg-with-1-dep'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  const reporter = sinon.spy()

  await addDependenciesToPackage(manifest, ['dep-of-pkg-with-1-dep@beta'], await testDefaults({ targetDependenciesField: 'devDependencies', reporter }))

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      dependencyType: 'dev',
      name: 'dep-of-pkg-with-1-dep',
      version: '100.0.0',
    },
  } as RootLog), 'reported old version removed from the root')

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: 'dep-of-pkg-with-1-dep',
      version: '100.1.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'reported new version added to the root')

  await project.has('dep-of-pkg-with-1-dep')
  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')

  t.equal(manifest.devDependencies!['dep-of-pkg-with-1-dep'], '^100.1.0')
})

test('scoped modules with versions', async (t) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['@zkochan/foo@1.0.0'], await testDefaults({ fastUnpack: false }))

  await project.has('@zkochan/foo')
})

test('multiple scoped modules (@rstacruz/...)', async (t) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], await testDefaults({ fastUnpack: false }))

  t.equal(typeof project.requireModule('@rstacruz/tap-spec'), 'function', 'tap-spec is available')
  t.equal(typeof project.requireModule('@rstacruz/travis-encrypt'), 'function', 'travis-encrypt is available')
})

test('nested scoped modules (test-pnpm-issue219 -> @zkochan/test-pnpm-issue219)', async (t) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['test-pnpm-issue219@1.0.2'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('test-pnpm-issue219')
  t.ok(m === 'test-pnpm-issue219,@zkochan/test-pnpm-issue219', 'nested scoped package is available')
})

test('idempotency', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()
  const opts = await testDefaults({ reporter })

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep@100.0.0'], opts)

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'pkg-with-1-dep',
      version: '100.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'reported that pkg-with-1-dep added to the root')

  reporter.resetHistory()

  await addDependenciesToPackage(manifest, ['pkg-with-1-dep@100.0.0'], opts)

  t.notOk(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'pkg-with-1-dep',
      version: '100.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'did not reported that pkg-with-1-dep was added because it was already there')

  await project.has('pkg-with-1-dep')
})

test('reporting adding root package', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults())

  await project.storeHas('flatten', '1.0.2')

  const reporter = sinon.spy()

  await addDependenciesToPackage(manifest, ['flatten@1.0.2'], await testDefaults({ reporter }))

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'flatten',
      version: '1.0.2',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'reported that flatten added to the root')
})

test('overwriting (magic-hook@2.0.0 and @0.1.0)', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults())

  await project.storeHas('flatten', '1.0.2')

  await addDependenciesToPackage(manifest, ['magic-hook@0.1.0'], await testDefaults())

  // flatten is not removed from store even though it is unreferenced
  // store should be pruned to have this removed
  await project.storeHas('flatten', '1.0.2')

  const m = project.requireModule('magic-hook/package.json')
  t.ok(m.version === '0.1.0', 'magic-hook is 0.1.0')
})

test('overwriting (is-positive@3.0.0 with is-positive@latest)', async (t) => {
  const project = prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['is-positive@3.0.0'], await testDefaults({ save: true }))

  await project.storeHas('is-positive', '3.0.0')

  const updatedManifest = await addDependenciesToPackage(manifest, ['is-positive@latest'], await testDefaults({ save: true }))

  await project.storeHas('is-positive', '3.1.0')
  t.equal(updatedManifest.dependencies?.['is-positive'], '3.1.0')
})

// Covers https://github.com/pnpm/pnpm/issues/2188
test('keeping existing specs untouched when adding new dependency', async (t) => {
  prepareEmpty(t)

  await addDistTag('bar', '100.1.0', 'latest')

  const manifest = await addDependenciesToPackage({ dependencies: { bar: '^100.0.0' } }, ['foo@100.1.0'], await testDefaults())

  t.deepEqual(manifest.dependencies, { bar: '^100.0.0', foo: '100.1.0' })
})

test('forcing', async (t) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false }))

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  await rimraf(distPath)

  await addDependenciesToPackage(manifest, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false, force: true }))

  const distPathExists = await exists(distPath)
  t.ok(distPathExists, 'magic-hook@2.0.0 dist folder reinstalled')
})

test('argumentless forcing', async (t: tape.Test) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false }))

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  await rimraf(distPath)

  await install(manifest, await testDefaults({ fastUnpack: false, force: true }))

  const distPathExists = await exists(distPath)
  t.ok(distPathExists, 'magic-hook@2.0.0 dist folder reinstalled')
})

test('no forcing', async (t) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults())

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  await rimraf(distPath)

  await addDependenciesToPackage(manifest, ['magic-hook@2.0.0'], await testDefaults())

  const distPathExists = await exists(distPath)
  t.notOk(distPathExists, 'magic-hook@2.0.0 dist folder not reinstalled')
})

test('refetch package to store if it has been modified', async (t) => {
  const project = prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false }))

  const distPathInStore = await project.resolve('magic-hook', '2.0.0', 'dist')
  await rimraf(distPathInStore)
  await rimraf('node_modules')
  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')

  await addDependenciesToPackage(manifest, ['magic-hook@2.0.0'], await testDefaults({ fastUnpack: false }))

  const distPathExists = await exists(distPath)
  t.ok(distPathExists, 'magic-hook@2.0.0 dist folder reinstalled')
})

// TODO: decide what to do with this case
// eslint-disable-next-line @typescript-eslint/dot-notation
test.skip('relink package to project if the dependency is not linked from store', async (t: tape.Test) => {
  prepareEmpty(t)
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

  t.ok(storeInode !== await getInode(), 'package.json inode changed')

  await install(manifest, await testDefaults({ repeatInstallDepth: 0 }))

  t.ok(storeInode === await getInode(), 'package.json inode matches the one that is in store')
})

test('circular deps', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['circular-deps-1-of-2'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('circular-deps-1-of-2/mirror')

  t.equal(m(), 'circular-deps-1-of-2', 'circular dependencies can access each other')

  t.notOk(await exists(path.join('node_modules', 'circular-deps-1-of-2', 'node_modules', 'circular-deps-2-of-2', 'node_modules', 'circular-deps-1-of-2')), 'circular dependency is avoided')
})

test('concurrent circular deps', async (t: tape.Test) => {
  // es5-ext is an external package from the registry
  // the latest dist-tag is overriden to have a stable test
  await addDistTag('es5-ext', '0.10.31', 'latest')
  await addDistTag('es6-iterator', '2.0.1', 'latest')

  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['es6-iterator@2.0.0'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('es6-iterator')

  t.ok(m, 'es6-iterator is installed')
  t.ok(await exists(path.resolve('node_modules/.pnpm/es6-iterator@2.0.0/node_modules/es5-ext')))
  t.ok(await exists(path.resolve('node_modules/.pnpm/es6-iterator@2.0.1/node_modules/es5-ext')))
  t.ok(await exists(path.resolve('node_modules/.pnpm/es5-ext@0.10.31/node_modules/es6-iterator')))
  t.ok(await exists(path.resolve('node_modules/.pnpm/es5-ext@0.10.31/node_modules/es6-symbol')))
})

test('concurrent installation of the same packages', async (t) => {
  const project = prepareEmpty(t)

  // the same version of core-js is required by two different dependencies
  // of babek-core
  await addDependenciesToPackage({}, ['babel-core@6.21.0'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('babel-core')

  t.ok(m, 'babel-core is installed')
})

test('big with dependencies and circular deps (babel-preset-2015)', async (t) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['babel-preset-es2015@6.3.13'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('babel-preset-es2015')
  t.ok(typeof m === 'object', 'babel-preset-es2015 is available')
})

test('bundledDependencies (pkg-with-bundled-dependencies@1.0.0)', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['pkg-with-bundled-dependencies@1.0.0'], await testDefaults({ fastUnpack: false }))

  await project.isExecutable('pkg-with-bundled-dependencies/node_modules/.bin/hello-world-js-bin')

  const lockfile = await project.readLockfile()
  t.deepEqual(
    lockfile.packages['/pkg-with-bundled-dependencies/1.0.0'].bundledDependencies,
    ['hello-world-js-bin'],
    `bundledDependencies added to ${WANTED_LOCKFILE}`
  )
})

test('bundleDependencies (pkg-with-bundle-dependencies@1.0.0)', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['pkg-with-bundle-dependencies@1.0.0'], await testDefaults({ fastUnpack: false }))

  await project.isExecutable('pkg-with-bundle-dependencies/node_modules/.bin/hello-world-js-bin')

  const lockfile = await project.readLockfile()
  t.deepEqual(
    lockfile.packages['/pkg-with-bundle-dependencies/1.0.0'].bundledDependencies,
    ['hello-world-js-bin'],
    `bundledDependencies added to ${WANTED_LOCKFILE}`
  )
})

test('installing a package with bundleDependencies set to false (pkg-with-bundle-dependencies-false)', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['pkg-with-bundle-dependencies-false'], await testDefaults({ fastUnpack: false }))

  const lockfile = await project.readLockfile()
  t.equal(
    typeof lockfile.packages['/pkg-with-bundle-dependencies-false/1.0.0'].bundledDependencies,
    'undefined'
  )
})

test('compiled modules (ursa@0.9.1)', async (t) => {
  // TODO: fix this for Node.js v7
  if (!isCI || IS_WINDOWS || semver.satisfies(process.version, '>=7.0.0')) {
    t.skip('runs only on CI')
    return
  }

  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['ursa@0.9.1'], await testDefaults())

  const m = project.requireModule('ursa')
  t.ok(typeof m === 'object', 'ursa() is available')
})

test('bin specified in the directories property linked to .bin folder', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['pkg-with-directories-bin'], await testDefaults({ fastUnpack: false }))

  await project.isExecutable('.bin/pkg-with-directories-bin')
})

test('building native addons', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['diskusage@1.1.3'], await testDefaults({ fastUnpack: false }))

  t.ok(await exists('node_modules/diskusage/build'), 'build folder created')

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/diskusage/1.1.3'].requiresBuild)
})

test('should update subdep on second install', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ save: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  let lockfile = await project.readLockfile()

  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'], 'lockfile has resolution for package')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  const reporter = sinon.spy()

  await install(manifest, await testDefaults({ depth: 1, update: true, reporter }))

  t.ok(reporter.calledWithMatch({
    added: 1,
    level: 'debug',
    name: 'pnpm:stats',
    prefix: process.cwd(),
  } as StatsLog), 'added stat')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')

  lockfile = await project.readLockfile()

  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'], "lockfile doesn't have old dependency")
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'], 'lockfile has new dependency')

  t.equal(deepRequireCwd(['pkg-with-1-dep', 'dep-of-pkg-with-1-dep', './package.json']).version, '100.1.0', 'updated in node_modules')
})

test('should not update subdep when depth is smaller than depth of package', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ save: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  let lockfile = await project.readLockfile()

  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'], 'lockfile has resolution for package')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(manifest, await testDefaults({ depth: 0, update: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  lockfile = await project.readLockfile()

  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'], 'lockfile has old dependency')
  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'], 'lockfile has not the new dependency')

  t.equal(deepRequireCwd(['pkg-with-1-dep', 'dep-of-pkg-with-1-dep', './package.json']).version, '100.0.0', 'not updated in node_modules')
})

test('should install dependency in second project', async (t) => {
  const project1 = prepareEmpty(t)

  await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ fastUnpack: false, save: true, store: '../store' }))
  t.equal(project1.requireModule('pkg-with-1-dep')().name, 'dep-of-pkg-with-1-dep', 'can require in 1st pkg')

  const project2 = prepareEmpty(t)

  await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ fastUnpack: false, save: true, store: '../store' }))

  t.equal(project2.requireModule('pkg-with-1-dep')().name, 'dep-of-pkg-with-1-dep', 'can require in 2nd pkg')
})

test('should throw error when trying to install using a different store then the previous one', async (t) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ storeDir: 'node_modules/.store1' }))

  try {
    await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ storeDir: 'node_modules/.store2' }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'ERR_PNPM_UNEXPECTED_STORE', 'failed with correct error code')
  }
})

test('ignores drive case in store path', async (t: tape.Test) => {
  if (!isWindows()) return

  prepareEmpty(t)

  // paths are case-insensitive on windows, so we will test with an upper and lower-case store
  const storePathUpper: string = path.resolve('node_modules/.store1').toUpperCase()
  const storePathLower: string = storePathUpper.toLowerCase()

  const manifest = await addDependenciesToPackage(
    {},
    ['rimraf@2.5.1'],
    await testDefaults({ storeDir: storePathUpper }, null, null, { ignoreFile: () => {} }) // eslint-disable-line:no-empty
  )
  await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ storeDir: storePathLower }))
  t.pass('Install did not fail')
})

test('should not throw error if using a different store after all the packages were uninstalled', async (t) => {
  // TODO: implement
})

test('should throw error when trying to install using a different virtual store directory then the previous one', async (t) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ virtualStoreDir: 'pkgs' }))

  let err: UnexpectedVirtualStoreDirError | null = null
  try {
    await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ virtualStoreDir: 'pnpm' }))
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_UNEXPECTED_VIRTUAL_STORE', 'failed with correct error code')
  t.equal(err.expected, path.resolve('pkgs'))
  t.equal(err.actual, path.resolve('pnpm'))
})

test('lockfile locks npm dependencies', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ save: true, reporter }))

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:progress',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep/100.0.0`,
    requester: process.cwd(),
    status: 'resolved',
  } as ProgressLog), 'logs that package is being resolved')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep/100.0.0`,
    requester: process.cwd(),
    status: 'fetched',
  } as ProgressLog), 'logged that package was fetched from registry')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await rimraf('node_modules')

  reporter.resetHistory()
  await install(manifest, await testDefaults({ reporter }))

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep/100.0.0`,
    requester: process.cwd(),
    status: 'resolved',
  } as ProgressLog), 'logs that package is being resolved')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    packageId: `localhost+${REGISTRY_MOCK_PORT}/pkg-with-1-dep/100.0.0`,
    requester: process.cwd(),
    status: 'found_in_store',
  } as ProgressLog), 'logged that package was found in store')

  const m = project.requireModule('.pnpm/pkg-with-1-dep@100.0.0/node_modules/dep-of-pkg-with-1-dep/package.json')

  t.equal(m.version, '100.0.0', `dependency specified in ${WANTED_LOCKFILE} is installed`)
})

test('self-require should work', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['uses-pkg-with-self-usage'], await testDefaults({ fastUnpack: false }))

  t.ok(project.requireModule('uses-pkg-with-self-usage'))
})

test('install on project with lockfile and no node_modules', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-negative'], await testDefaults())

  await rimraf('node_modules')

  await addDependenciesToPackage(manifest, ['is-positive'], await testDefaults())

  await project.has('is-positive') // installed new dependency

  // We have to install all other direct dependencies in case they resolve some peers
  await project.has('is-negative')
})

test('install a dependency with * range', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  await install({
    dependencies: {
      'has-beta-only': '*',
    },
  }, await testDefaults({ reporter }))

  await project.has('has-beta-only')

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: {
      dependencies: {
        'has-beta-only': '*',
      },
    } as ProjectManifest,
  } as PackageManifestLog), 'should log package-json updated even when package.json was not changed')
})

test('should throw error when trying to install a package without name', async (t: tape.Test) => {
  prepareEmpty(t)
  try {
    await addDependenciesToPackage({}, [`file:${pathToLocalPkg('missing-pkg-name.tgz')}`], await testDefaults())
    t.fail('installation should have failed')
  } catch (err) {
    if (err.message.match(/^Can't install .*: Missing package name$/)) {
      t.pass('correct error message')
    } else {
      t.fail(`incorrect error message "${err.message as string}"`)
    }
    t.equal(err.code, 'ERR_PNPM_MISSING_PACKAGE_NAME', 'failed with correct error code')
  }
})

// Covers https://github.com/pnpm/pnpm/issues/1193
test('rewrites node_modules created by npm', async (t) => {
  const project = prepareEmpty(t)

  await execa('npm', ['install', 'rimraf@2.5.1', '@types/node', '--save'])

  const manifest = await install({}, await testDefaults())

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')

  await execa('npm', ['install', '-f', 'rimraf@2.5.1', '@types/node', '--save'])

  await install(manifest, await testDefaults())
})

// Covers https://github.com/pnpm/pnpm/issues/1685
// also, there's a better version of this test (with the same name) in the pnpm package
// TODO: move this test to @pnpm/package-store
test("don't fail on case insensitive filesystems when package has 2 files with same name", async (t) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  const opts = await testDefaults({ reporter })
  await addDependenciesToPackage({}, ['with-same-file-in-different-cases'], opts)

  await project.has('with-same-file-in-different-cases')
})

// Covers https://github.com/pnpm/pnpm/issues/1134
test('reinstalls missing packages to node_modules', async (t) => {
  prepareEmpty(t)
  const reporter = sinon.spy()
  const depLocation = path.resolve('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive')
  const missingDepLog = {
    level: 'debug',
    missing: depLocation,
    name: 'pnpm:_broken_node_modules',
  }

  const opts = await testDefaults({ fastUnpack: false, reporter })
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], opts)

  t.notOk(reporter.calledWithMatch(missingDepLog))

  await rimraf('pnpm-lock.yaml')
  await rimraf(depLocation)

  let err
  try {
    await import(path.resolve('node_modules/is-positive'))
  } catch (_err) {
    err = _err
  }

  t.ok(err)

  reporter.resetHistory()

  await install(manifest, opts)

  t.ok(reporter.calledWithMatch(missingDepLog))
  t.ok(await import(path.resolve('node_modules/is-positive')))
})

// Covers https://github.com/pnpm/pnpm/issues/1134
test('reinstalls missing packages to node_modules during headless install', async (t) => {
  prepareEmpty(t)
  const reporter = sinon.spy()
  const depLocation = path.resolve('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive')
  const missingDepLog = {
    level: 'debug',
    missing: depLocation,
    name: 'pnpm:_broken_node_modules',
  }

  const opts = await testDefaults({ fastUnpack: false, reporter })
  const manifest = await addDependenciesToPackage({}, ['is-positive@1.0.0'], opts)

  t.notOk(reporter.calledWithMatch(missingDepLog))

  await rimraf(depLocation)

  let err
  try {
    await import(path.resolve('node_modules/is-positive'))
  } catch (_err) {
    err = _err
  }

  t.ok(err)

  reporter.resetHistory()

  await install(manifest, opts)

  t.ok(reporter.calledWithMatch(missingDepLog))
  t.ok(await import(path.resolve('node_modules/is-positive')))
})

test('do not update deps when lockfile is present', async (t) => {
  await addDistTag('peer-a', '1.0.0', 'latest')
  const project = prepareEmpty(t)

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

  t.deepEqual(initialLockfile, latestLockfile)
})

test('all the subdeps of dependencies are linked when a node_modules is partially up-to-date', async (t: tape.Test) => {
  prepareEmpty(t)

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
  })

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

  t.deepEqual(
    await fs.readdir(path.resolve('node_modules/.pnpm/foobarqar@1.0.1/node_modules')),
    [
      'bar',
      'foo',
      'foobarqar',
      'qar',
    ]
  )
})

test('subdep symlinks are updated if the lockfile has new subdep versions specified', async (t: tape.Test) => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')
  const project = prepareEmpty(t)

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

  t.deepEqual(
    Object.keys(lockfile.packages),
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
  })

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

  t.ok(await exists(path.resolve('node_modules/.pnpm/pkg-with-1-dep@100.0.0/node_modules/dep-of-pkg-with-1-dep/package.json')))
})

test('fail if none of the available resolvers support a version spec', async (t: tape.Test) => {
  prepareEmpty(t)

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
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER')
  t.deepEqual(
    err.pkgsStack,
    [
      {
        id: `localhost+${REGISTRY_MOCK_PORT}/@types/plotly.js/1.44.29`,
        name: '@types/plotly.js',
        version: '1.44.29',
      },
    ]
  )
})

test('globally installed package which don\'t have bins should log warning message', async (t: tape.Test) => {
  prepareEmpty(t)
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

  t.ok(reporter.calledWithMatch({
    message: 'is-positive has no binaries',
    prefix: process.cwd(),
  }))

  t.end()
})

// Covers issue: https://github.com/pnpm/pnpm/issues/2629
test('installing a package that has a manifest with byte order mark (BOM)', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      paralleljs: '0.2.1',
    },
  }, await testDefaults())

  await project.has('paralleljs')
})

test('ignore files in node_modules', async (t: tape.Test) => {
  const project = prepareEmpty(t)
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
  t.ok(typeof m === 'function', '_ is available')
  t.ok(typeof m.clone === 'function', '_.clone is available')
  t.equal(await fs.readFile('node_modules/foo', 'utf8'), 'x')
})

// Covers https://github.com/pnpm/pnpm/issues/2339
test('memory consumption is under control on huge package with many peer dependencies. Sample 1', async (t: tape.Test) => {
  prepareEmpty(t)

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['@teambit/bit@0.0.30'],
    await testDefaults({ fastUnpack: true, lockfileOnly: true })
  )

  t.ok(await exists('pnpm-lock.yaml'), 'lockfile created')
})

// Covers https://github.com/pnpm/pnpm/issues/2339
test('memory consumption is under control on huge package with many peer dependencies. Sample 2', async (t: tape.Test) => {
  prepareEmpty(t)

  await addDependenciesToPackage(
    {
      name: 'project',
      version: '0.0.0',
    },
    ['@teambit/react@0.0.30'],
    await testDefaults({ fastUnpack: true, lockfileOnly: true })
  )

  t.ok(await exists('pnpm-lock.yaml'), 'lockfile created')
})

test('installing with no symlinks with PnP', async (t) => {
  const project = prepareEmpty(t)

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

  t.deepEqual(await fs.readdir(path.resolve('node_modules')), ['.bin', '.modules.yaml', '.pnpm'])
  t.deepEqual(await fs.readdir(path.resolve('node_modules/.pnpm/rimraf@2.7.1/node_modules')), ['rimraf'])

  t.ok(await project.readCurrentLockfile())
  t.ok(await project.readModulesManifest())
  t.ok(await exists(path.resolve('.pnp.js')))

  t.end()
})
