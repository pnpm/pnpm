import 'sepia'
import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import fs = require('mz/fs')
import caw = require('caw')
import semver = require('semver')
import crossSpawn = require('cross-spawn')
const spawnSync = crossSpawn.sync
import isCI = require('is-ci')
import rimraf = require('rimraf-then')
import readPkg = require('read-pkg')
import {
  prepare,
  addDistTag,
  testDefaults,
  execPnpmSync,
} from '../utils'
import loadJsonFile = require('load-json-file')
const basicPackageJson = loadJsonFile.sync(path.join(__dirname, '../utils/simple-package.json'))
import {install, installPkgs, uninstall} from '../../src'
import exists = require('path-exists')
import isWindows = require('is-windows')
import deepRequireCwd = require('deep-require-cwd')

const IS_WINDOWS = isWindows()

if (!caw() && !IS_WINDOWS) {
  process.env.VCR_MODE = 'cache'
}

test('small with dependencies (rimraf)', async function (t) {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1'], testDefaults())

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')
})

test('no dependencies (lodash)', async function (t) {
  const project = prepare(t)
  await installPkgs(['lodash@4.0.0'], testDefaults())

  const m = project.requireModule('lodash')
  t.ok(typeof m === 'function', '_ is available')
  t.ok(typeof m.clone === 'function', '_.clone is available')
})

test('scoped modules without version spec (@rstacruz/tap-spec)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec'], testDefaults())

  const m = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof m === 'function', 'tap-spec is available')
})

test('scoped package with custom registry', async function (t) {
  const project = prepare(t)

  await installPkgs(['@scoped/peer'], testDefaults({
    // setting an incorrect default registry URL
    registry: 'http://localhost:9999/',
    rawNpmConfig: {
      '@scoped:registry': 'http://localhost:4873/',
    },
  }))

  const m = project.requireModule('@scoped/peer/package.json')
  t.ok(m, 'is available')
})

test('modules without version spec, with custom tag config', async function (t) {
  const project = prepare(t)

  const tag = 'beta'

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', tag)

  await installPkgs(['dep-of-pkg-with-1-dep'], testDefaults({tag}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('installing a package by specifying a specific dist-tag', async function (t) {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'beta')

  await installPkgs(['dep-of-pkg-with-1-dep@beta'], testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('scoped modules with versions (@rstacruz/tap-spec@4.1.1)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec@4.1.1'], testDefaults())

  const m = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof m === 'function', 'tap-spec is available')
})

test('scoped modules (@rstacruz/tap-spec@*)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec@*'], testDefaults())

  const m = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof m === 'function', 'tap-spec is available')
})

test('multiple scoped modules (@rstacruz/...)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], testDefaults())

  t.equal(typeof project.requireModule('@rstacruz/tap-spec'), 'function', 'tap-spec is available')
  t.equal(typeof project.requireModule('@rstacruz/travis-encrypt'), 'function', 'travis-encrypt is available')
})

test('nested scoped modules (test-pnpm-issue219 -> @zkochan/test-pnpm-issue219)', async function (t) {
  const project = prepare(t)
  await installPkgs(['test-pnpm-issue219@1.0.2'], testDefaults())

  const m = project.requireModule('test-pnpm-issue219')
  t.ok(m === 'test-pnpm-issue219,@zkochan/test-pnpm-issue219', 'nested scoped package is available')
})

test('idempotency (rimraf)', async function (t) {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1'], testDefaults())
  await installPkgs(['rimraf@2.5.1'], testDefaults())

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf is available')
})

test('overwriting (magic-hook@2.0.0 and @0.1.0)', async function (t) {
  const project = prepare(t)
  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  await project.storeHas('flatten', '1.0.2')

  await installPkgs(['magic-hook@0.1.0'], testDefaults())

  await project.storeHasNot('flatten', '1.0.2')

  const m = project.requireModule('magic-hook/package.json')
  t.ok(m.version === '0.1.0', 'magic-hook is 0.1.0')
})

test('overwriting (is-positive@3.0.0 with is-positive@latest)', async function (t) {
  const project = prepare(t)
  await installPkgs(['is-positive@3.0.0'], testDefaults({save: true}))

  await project.storeHas('is-positive', '3.0.0')

  await installPkgs(['is-positive@latest'], testDefaults({save: true}))

  await project.storeHas('is-positive', '3.1.0')
})

test('forcing', async function (t) {
  const project = prepare(t)
  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  await rimraf(distPath)

  await installPkgs(['magic-hook@2.0.0'], testDefaults({force: true}))

  const distPathExists = await exists(distPath)
  t.ok(distPathExists, 'magic-hook@2.0.0 dist folder reinstalled')
})

test('no forcing', async function (t) {
  const project = prepare(t)
  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  await rimraf(distPath)

  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPathExists = await exists(distPath)
  t.ok(!distPathExists, 'magic-hook@2.0.0 dist folder not reinstalled')
})

test('refetch package to store if it has been modified', async function (t) {
  const project = prepare(t)
  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPathInStore = await project.resolve('magic-hook', '2.0.0', 'dist')
  await rimraf(distPathInStore)
  await rimraf('node_modules')
  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')

  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPathExists = await exists(distPath)
  t.ok(distPathExists, 'magic-hook@2.0.0 dist folder reinstalled')
})

// TODO: decide what to do with this case
test['skip']('relink package to project if the dependency is not linked from store', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['magic-hook@2.0.0'], testDefaults({save: true, saveExact: true}))

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

  await install(testDefaults({repeatInstallDepth: 0}))

  t.ok(storeInode === await getInode(), 'package.json inode matches the one that is in store')
})

test('circular deps', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['circular-deps-1-of-2'], testDefaults())

  const m = project.requireModule('circular-deps-1-of-2/mirror')

  t.equal(m(), 'circular-deps-1-of-2', 'circular dependencies can access each other')

  t.ok(!await exists(path.join('node_modules', 'circular-deps-1-of-2', 'node_modules', 'circular-deps-2-of-2', 'node_modules', 'circular-deps-1-of-2')), 'circular dependency is avoided')
})

test('concurrent circular deps', async function (t) {
  const project = prepare(t)
  await installPkgs(['es6-iterator@2.0.0'], testDefaults())

  const m = project.requireModule('es6-iterator')

  t.ok(m, 'es6-iterator is installed')
  t.ok(await exists(path.join('node_modules', '.localhost+4873', 'es6-iterator', '2.0.0', 'node_modules', 'es5-ext')))
  t.ok(await exists(path.join('node_modules', '.localhost+4873', 'es6-iterator', '2.0.1', 'node_modules', 'es5-ext')))
})

test('concurrent installation of the same packages', async function (t) {
  const project = prepare(t)

  // the same version of core-js is required by two different dependencies
  // of babek-core
  await installPkgs(['babel-core@6.21.0'], testDefaults())

  const m = project.requireModule('babel-core')

  t.ok(m, 'babel-core is installed')
})

test('big with dependencies and circular deps (babel-preset-2015)', async function (t) {
  const project = prepare(t)
  await installPkgs(['babel-preset-es2015@6.3.13'], testDefaults())

  const m = project.requireModule('babel-preset-es2015')
  t.ok(typeof m === 'object', 'babel-preset-es2015 is available')
})

test('bundleDependencies (pkg-with-bundled-dependencies@1.0.0)', async function (t) {
  const project = prepare(t)
  await installPkgs(['pkg-with-bundled-dependencies@1.0.0'], testDefaults())

  await project.isExecutable('pkg-with-bundled-dependencies/node_modules/.bin/hello-world-js-bin')
})

test('compiled modules (ursa@0.9.1)', async function (t) {
  // TODO: fix this for Node.js v7
  if (!isCI || IS_WINDOWS || semver.satisfies(process.version, '>=7.0.0')) {
    t.skip('runs only on CI')
    return
  }

  const project = prepare(t)
  await installPkgs(['ursa@0.9.1'], testDefaults())

  const m = project.requireModule('ursa')
  t.ok(typeof m === 'object', 'ursa() is available')
})

test('shrinkwrap compatibility', async function (t) {
  if (semver.satisfies(process.version, '4')) {
    t.skip("don't run on Node.js 4")
    return
  }
  const project = prepare(t, { dependencies: { rimraf: '*' } })

  await installPkgs(['rimraf@2.5.1'], testDefaults())

  return new Promise((resolve, reject) => {
    const proc = crossSpawn.spawn('npm', ['shrinkwrap'])

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      const wrap = JSON.parse(fs.readFileSync('npm-shrinkwrap.json', 'utf-8'))
      t.ok(wrap.dependencies.rimraf.version === '2.5.1',
        'npm shrinkwrap is successful')
      resolve()
    })
  })
})

test('save to package.json (rimraf@2.5.1)', async function (t) {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1'], testDefaults({ save: true }))

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, {rimraf: '^2.5.1'}, 'rimraf has been added to dependencies')
})

test('saveDev scoped module to package.json (@rstacruz/tap-spec)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec'], testDefaults({ saveDev: true }))

  const m = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof m === 'function', 'tapSpec() is available')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.devDependencies, { '@rstacruz/tap-spec': '^4.1.1' }, 'tap-spec has been added to devDependencies')
})

test('multiple save to package.json with `exact` versions (@rstacruz/tap-spec & rimraf@2.5.1) (in sorted order)', async function (t) {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1', '@rstacruz/tap-spec@latest'], testDefaults({ save: true, saveExact: true }))

  const m1 = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof m1 === 'function', 'tapSpec() is available')

  const m2 = project.requireModule('rimraf')
  t.ok(typeof m2 === 'function', 'rimraf() is available')

  const pkgJson = await readPkg()
  const expectedDeps = {
    '@rstacruz/tap-spec': '4.1.1',
    rimraf: '2.5.1'
  }
  t.deepEqual(pkgJson.dependencies, expectedDeps, 'tap-spec and rimraf have been added to dependencies')
  t.deepEqual(Object.keys(pkgJson.dependencies), Object.keys(expectedDeps), 'tap-spec and rimraf have been added to dependencies in sorted order')
})

test('production install (with --production flag)', async function (t) {
  const project = prepare(t, basicPackageJson)

  await install(testDefaults({ production: true }))

  const rimrafDir = fs.statSync(path.resolve('node_modules', 'rimraf'))

  let tapStatErrCode: number = 0
  try {
    fs.statSync(path.resolve('node_modules', '@rstacruz'))
  } catch (err) {
    tapStatErrCode = err.code
  }

  t.ok(rimrafDir.isSymbolicLink, 'rimraf exists')
  t.is(tapStatErrCode, 'ENOENT', 'tap-spec does not exist')
})

test('production install (with production NODE_ENV)', async function (t) {
  const originalNodeEnv = process.env.NODE_ENV
  process.env.NODE_ENV = 'production'
  const project = prepare(t, basicPackageJson)

  await install(testDefaults())

  // reset NODE_ENV
  process.env.NODE_ENV = originalNodeEnv

  const rimrafDir = fs.statSync(path.resolve('node_modules', 'rimraf'))

  let tapStatErrCode: number = 0
  try {
    fs.statSync(path.resolve('node_modules', '@rstacruz'))
  } catch (err) { tapStatErrCode = err.code }

  t.ok(rimrafDir.isSymbolicLink, 'rimraf exists')
  t.is(tapStatErrCode, 'ENOENT', 'tap-spec does not exist')
})

const wait = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

test('support installing into the same store simultaneously', async t => {
  const project = prepare(t)
  await Promise.all([
    installPkgs(['pkg-that-installs-slowly'], testDefaults()),
    wait(500) // to be sure that lock was created
      .then(async () => {
        await project.storeHasNot('pkg-that-installs-slowly')
        await installPkgs(['rimraf@2.5.1'], testDefaults())
      })
      .then(async  () => {
        await project.has('pkg-that-installs-slowly')
        await project.has('rimraf')
      })
      .catch(err => t.notOk(err))
  ])
})

test('support installing and uninstalling from the same store simultaneously', async t => {
  const project = prepare(t)
  await Promise.all([
    installPkgs(['pkg-that-installs-slowly'], testDefaults()),
    wait(500) // to be sure that lock was created
      .then(async () => {
        await project.storeHasNot('pkg-that-installs-slowly')
        await uninstall(['rimraf@2.5.1'], testDefaults())
      })
      .then(async () => {
        await project.has('pkg-that-installs-slowly')
        await project.hasNot('rimraf')
      })
      .catch(err => t.notOk(err))
  ])
})

test('top-level packages should find the plugins they use', async function (t) {
  const project = prepare(t, {
    scripts: {
      test: 'pkg-that-uses-plugins'
    }
  })
  await installPkgs(['pkg-that-uses-plugins', 'plugin-example'], testDefaults({ save: true }))
  const result = spawnSync('npm', ['test'])
  t.ok(result.stdout.toString().indexOf('My plugin is plugin-example') !== -1, 'package executable have found its plugin')
  t.equal(result.status, 0, 'executable exited with success')
})

test('not top-level packages should find the plugins they use', async function (t) {
  // standard depends on eslint and eslint plugins
  const project = prepare(t, {
    scripts: {
      test: 'standard'
    }
  })
  await installPkgs(['standard@8.6.0'], testDefaults({ save: true }))
  const result = spawnSync('npm', ['test'])
  console.log(result.stdout.toString())
  t.equal(result.status, 0, 'standard exited with success')
})

test('bin specified in the directories property linked to .bin folder', async function (t) {
  const project = prepare(t)

  await installPkgs(['pkg-with-directories-bin'], testDefaults())

  await project.isExecutable('.bin/pkg-with-directories-bin')
})

test('run js bin file', async function (t) {
  const project = prepare(t, {
    scripts: {
      test: 'hello-world-js-bin'
    }
  })
  await installPkgs(['hello-world-js-bin'], testDefaults({ save: true }))

  const result = spawnSync('npm', ['test'])
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'package executable printed its message')
  t.equal(result.status, 0, 'executable exited with success')
})

test('bin files are found by lifecycle scripts', t => {
  const project = prepare(t, {
    scripts: {
      postinstall: 'hello-world-js-bin'
    },
    dependencies: {
      'hello-world-js-bin': '*'
    }
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'postinstall script was executed')

  t.end()
})

test('global installation', async function (t) {
  prepare(t)
  const globalPrefix = path.resolve('..', 'global')
  const opts = testDefaults({global: true, prefix: globalPrefix})
  await installPkgs(['is-positive'], opts)

  const isPositive = require(path.join(globalPrefix, 'node_modules', 'is-positive'))
  t.ok(typeof isPositive === 'function', 'isPositive() is available')
})

test('create a pnpm-debug.log file when the command fails', async function (t) {
  const project = prepare(t)

  const result = execPnpmSync('install', '@zkochan/i-do-not-exist')

  t.equal(result.status, 1, 'install failed')

  t.ok(await exists('pnpm-debug.log'), 'log file created')

  t.end()
})

test('building native addons', async function (t) {
  const project = prepare(t)

  await installPkgs(['runas@3.1.1'], testDefaults())

  t.ok(await exists(path.join('node_modules', 'runas', 'build')), 'build folder created')
})

test('should update subdep on second install', async function (t) {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  let shr = await project.loadShrinkwrap()

  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'], 'shrinkwrap has resolution for package')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(testDefaults({depth: 1, update: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')

  shr = await project.loadShrinkwrap()

  t.ok(!shr.packages['/dep-of-pkg-with-1-dep/100.0.0'], "shrinkwrap doesn't have old dependency")
  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.1.0'], 'shrinkwrap has new dependency')

  t.equal(deepRequireCwd(['pkg-with-1-dep', 'dep-of-pkg-with-1-dep', './package.json']).version, '100.1.0', 'updated in node_modules')
})

test('should install dependency in second project', async function (t) {
  const project1 = prepare(t)

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true, storePath: '../store'}))
  t.equal(project1.requireModule('pkg-with-1-dep')().name, 'dep-of-pkg-with-1-dep', 'can require in 1st pkg')

  const project2 = prepare(t)

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true, storePath: '../store'}))

  t.equal(project2.requireModule('pkg-with-1-dep')().name, 'dep-of-pkg-with-1-dep', 'can require in 2nd pkg')
})

test('should throw error when trying to install using a different store then the previous one', async function(t) {
  const project = prepare(t)

  await installPkgs(['rimraf@2.5.1'], testDefaults({storePath: 'node_modules/.store1'}))

  try {
    await installPkgs(['is-negative'], testDefaults({storePath: 'node_modules/.store2'}))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'UNEXPECTED_STORE', 'failed with correct error code')
  }
})

test('should not throw error if using a different store after all the packages were uninstalled', async function(t) {
  // TODO: implement
})

test('shrinkwrap locks npm dependencies', async function (t) {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await rimraf('node_modules')

  await install(testDefaults({}))

  const m = project.requireModule('.localhost+4873/pkg-with-1-dep/100.0.0/node_modules/dep-of-pkg-with-1-dep/package.json')

  t.equal(m.version, '100.0.0', 'dependency specified in shrinkwrap.yaml is installed')
})

test('self-require should work', async function (t) {
  const project = prepare(t)

  await installPkgs(['uses-pkg-with-self-usage'], testDefaults())

  t.ok(project.requireModule('uses-pkg-with-self-usage'))
})
