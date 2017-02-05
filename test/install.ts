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
import {add as addDistTag} from './support/distTags'
import prepare from './support/prepare'
import loadJsonFile = require('load-json-file')
const basicPackageJson = loadJsonFile.sync(path.join(__dirname, './support/simple-package.json'))
import {install, installPkgs, uninstall} from '../src'
import testDefaults from './support/testDefaults'
import exists = require('exists-file')
import {pathToLocalPkg, local} from './support/localPkg'
import {sync as execPnpmSync} from './support/execPnpm'
import isWindows = require('is-windows')

const IS_WINDOWS = isWindows()

if (!caw() && !IS_WINDOWS) {
  process.env.VCR_MODE = 'cache'
}

test('small with dependencies (rimraf)', async function (t) {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1'], testDefaults())

  const rimraf = project.requireModule('rimraf')
  t.ok(typeof rimraf === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')
})

test('no dependencies (lodash)', async function (t) {
  const project = prepare(t)
  await installPkgs(['lodash@4.0.0'], testDefaults())

  const _ = project.requireModule('lodash')
  t.ok(typeof _ === 'function', '_ is available')
  t.ok(typeof _.clone === 'function', '_.clone is available')
})

test('scoped modules without version spec (@rstacruz/tap-spec)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec'], testDefaults())

  const _ = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof _ === 'function', 'tap-spec is available')
})

test('scoped modules with versions (@rstacruz/tap-spec@4.1.1)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec@4.1.1'], testDefaults())

  const _ = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof _ === 'function', 'tap-spec is available')
})

test('scoped modules (@rstacruz/tap-spec@*)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec@*'], testDefaults())

  const _ = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof _ === 'function', 'tap-spec is available')
})

test('multiple scoped modules (@rstacruz/...)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], testDefaults())

  const tapSpec = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof tapSpec === 'function', 'tap-spec is available')

  const travisEncrypt = project.requireModule('@rstacruz/travis-encrypt')
  t.ok(typeof travisEncrypt === 'function', 'travis-encrypt is available')
})

test('nested scoped modules (test-pnpm-issue219 -> @zkochan/test-pnpm-issue219)', async function (t) {
  const project = prepare(t)
  await installPkgs(['test-pnpm-issue219@1.0.2'], testDefaults())

  const _ = project.requireModule('test-pnpm-issue219')
  t.ok(_ === 'test-pnpm-issue219,@zkochan/test-pnpm-issue219', 'nested scoped package is available')
})

test('scoped modules from a directory', async function (t) {
  const project = prepare(t)
  await installPkgs([local('local-scoped-pkg')], testDefaults())

  const localPkg = project.requireModule('@scope/local-scoped-pkg')

  t.equal(localPkg(), '@scope/local-scoped-pkg', 'localScopedPkg() is available')
})

test('skip failing optional dependencies', async function (t) {
  const project = prepare(t)
  await installPkgs(['pkg-with-failing-optional-dependency@1.0.1'], testDefaults())

  const isNegative = project.requireModule('pkg-with-failing-optional-dependency')
  t.ok(isNegative(-1), 'package with failed optional dependency has the dependencies installed correctly')
})

test('skip optional dependency that does not support the current OS', async function (t) {
  const project = prepare(t, {
    optionalDependencies: {
      'not-compatible-with-any-os': '*'
    }
  })
  await install(testDefaults())

  await project.hasNot('not-compatible-with-any-os')
  await project.storeHasNot('not-compatible-with-any-os', '1.0.0')
})

test('skip optional dependency that does not support the current Node version', async function (t) {
  const project = prepare(t, {
    optionalDependencies: {
      'for-legacy-node': '*'
    }
  })

  await install(testDefaults())

  await project.hasNot('for-legacy-node')
  await project.storeHasNot('for-legacy-node', '1.0.0')
})

test('skip optional dependency that does not support the current pnpm version', async function (t) {
  const project = prepare(t, {
    optionalDependencies: {
      'for-legacy-pnpm': '*'
    }
  })

  await install(testDefaults())

  await project.hasNot('for-legacy-pnpm')
  await project.storeHasNot('for-legacy-pnpm', '1.0.0')
})

test('don\'t skip optional dependency that does not support the current OS when forcing', async function (t) {
  const project = prepare(t, {
    optionalDependencies: {
      'not-compatible-with-any-os': '*'
    }
  })

  await install(testDefaults({
    force: true
  }))

  await project.has('not-compatible-with-any-os')
  await project.storeHas('not-compatible-with-any-os', '1.0.0')
})

test('fail if installed package does not support the current engine and engine-strict = true', async function (t) {
  const project = prepare(t)

  try {
    await installPkgs(['not-compatible-with-any-os'], testDefaults({
      engineStrict: true
    }))
    t.fail()
  } catch (err) {
    await project.hasNot('not-compatible-with-any-os')
    await project.storeHasNot('not-compatible-with-any-os', '1.0.0')
  }
})

test('do not fail if installed package does not support the current engine and engine-strict = false', async function (t) {
  const project = prepare(t)

  await installPkgs(['not-compatible-with-any-os'], testDefaults({
    engineStrict: false
  }))

  await project.has('not-compatible-with-any-os')
  await project.storeHas('not-compatible-with-any-os', '1.0.0')
})

test('do not fail if installed package requires the node version that was passed in and engine-strict = true', async function (t) {
  const project = prepare(t)

  await installPkgs(['for-legacy-node'], testDefaults({
    engineStrict: true,
    nodeVersion: '0.10.0'
  }))

  await project.has('for-legacy-node')
  await project.storeHas('for-legacy-node', '1.0.0')
})

test('idempotency (rimraf)', async function (t) {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1'], testDefaults())
  await installPkgs(['rimraf@2.5.1'], testDefaults())

  const rimraf = project.requireModule('rimraf')
  t.ok(typeof rimraf === 'function', 'rimraf is available')
})

test('overwriting (magic-hook@2.0.0 and @0.1.0)', async function (t) {
  const project = prepare(t)
  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  await project.storeHas('flatten', '1.0.2')

  await installPkgs(['magic-hook@0.1.0'], testDefaults())

  await project.storeHasNot('flatten', '1.0.2')

  const _ = project.requireModule('magic-hook/package.json')
  t.ok(_.version === '0.1.0', 'magic-hook is 0.1.0')
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

test('relink package to project if it has been refetched', async function (t) {
  const project = prepare(t)
  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPathInStore = await project.resolve('magic-hook', '2.0.0', 'dist')
  await rimraf(distPathInStore)
  const distPath = path.resolve('node_modules', 'magic-hook', 'dist')
  await rimraf(distPath)

  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPathExists = await exists(distPath)
  t.ok(distPathExists, 'magic-hook@2.0.0 dist folder reinstalled')
})

test('relink package to project if the dependency is not linked from store', async function (t) {
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

  await install(testDefaults())

  t.ok(storeInode === await getInode(), 'package.json inode matches the one that is in store')
})

test('circular deps', async function (t) {
  const project = prepare(t)
  await installPkgs(['circular-deps-1-of-2'], testDefaults())

  const dep = project.requireModule('circular-deps-1-of-2/mirror')

  t.equal(dep(), 'circular-deps-1-of-2', 'circular dependencies can access each other')

  t.ok(!await exists(path.join('node_modules', 'circular-deps-1-of-2', 'node_modules', 'circular-deps-2-of-2', 'node_modules', 'circular-deps-1-of-2')), 'circular dependency is avoided')
})

test('concurrent circular deps', async function (t) {
  const project = prepare(t)
  await installPkgs(['es6-iterator@2.0.0'], testDefaults())

  const dep = project.requireModule('es6-iterator')

  t.ok(dep, 'es6-iterator is installed')
})

test('concurrent installation of the same packages', async function (t) {
  const project = prepare(t)

  // the same version of core-js is required by two different dependencies
  // of babek-core
  await installPkgs(['babel-core@6.21.0'], testDefaults())

  const dep = project.requireModule('babel-core')

  t.ok(dep, 'babel-core is installed')
})

test('big with dependencies and circular deps (babel-preset-2015)', async function (t) {
  const project = prepare(t)
  await installPkgs(['babel-preset-es2015@6.3.13'], testDefaults())

  const b = project.requireModule('babel-preset-es2015')
  t.ok(typeof b === 'object', 'babel-preset-es2015 is available')
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

  const ursa = project.requireModule('ursa')
  t.ok(typeof ursa === 'object', 'ursa() is available')
})

test('tarballs (is-array-1.0.1.tgz)', async function (t) {
  const project = prepare(t)
  await installPkgs(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], testDefaults())

  const isArray = project.requireModule('is-array')

  t.ok(isArray, 'isArray() is available')

  await project.storeHas('is-array-1.0.1#a83102a9c117983e6ff4d85311fb322231abe3d6')
})

test('tarballs from GitHub (is-negative)', async function (t) {
  const project = prepare(t)
  await installPkgs(['is-negative@https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz'], testDefaults())

  const isNegative = project.requireModule('is-negative')

  t.ok(isNegative, 'isNegative() is available')
})

test('local file', async function (t) {
  const project = prepare(t)
  await installPkgs([local('local-pkg')], testDefaults())

  const localPkg = project.requireModule('local-pkg')

  t.ok(localPkg, 'localPkg() is available')
})

test('package with a broken symlink', async function (t) {
  const project = prepare(t)
  await installPkgs([pathToLocalPkg('has-broken-symlink/has-broken-symlink.tar.gz')], testDefaults())

  const pkg = project.requireModule('has-broken-symlink')

  t.ok(pkg, 'has-broken-symlink is available')
})

test('nested local dependency of a local dependency', async function (t) {
  const project = prepare(t)
  await installPkgs([local('pkg-with-local-dep')], testDefaults())

  const pkgWithLocalDep = project.requireModule('pkg-with-local-dep')

  t.ok(pkgWithLocalDep, 'pkgWithLocalDep() is available')

  t.equal(pkgWithLocalDep(), 'local-pkg', 'pkgWithLocalDep() returns data from local-pkg')
})

test('from a github repo', async function (t) {
  const project = prepare(t)
  await installPkgs(['kevva/is-negative'], testDefaults())

  const localPkg = project.requireModule('is-negative')

  t.ok(localPkg, 'isNegative() is available')
})

test('from a git repo', async function (t) {
  if (isCI) {
    t.skip('not testing the SSH GIT access via CI')
    return t.end()
  }
  const project = prepare(t)
  await installPkgs(['git+ssh://git@github.com/kevva/is-negative.git'], testDefaults())

  const localPkg = project.requireModule('is-negative')

  t.ok(localPkg, 'isNegative() is available')
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

test('run pre/postinstall scripts', async function (t) {
  const project = prepare(t)
  await installPkgs(['pre-and-postinstall-scripts-example'], testDefaults())

  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
})

test('run install scripts', async function (t) {
  const project = prepare(t)
  await installPkgs(['install-script-example'], testDefaults())

  const generatedByInstall = project.requireModule('install-script-example/generated-by-install')
  t.ok(typeof generatedByInstall === 'function', 'generatedByInstall() is available')
})

test('save to package.json (rimraf@2.5.1)', async function (t) {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1'], testDefaults({ save: true }))

  const rimraf = project.requireModule('rimraf')
  t.ok(typeof rimraf === 'function', 'rimraf() is available')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  t.deepEqual(dependencies, {rimraf: '^2.5.1'}, 'rimraf has been added to dependencies')
})

test('saveDev scoped module to package.json (@rstacruz/tap-spec)', async function (t) {
  const project = prepare(t)
  await installPkgs(['@rstacruz/tap-spec'], testDefaults({ saveDev: true }))

  const tapSpec = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof tapSpec === 'function', 'tapSpec() is available')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const devDependencies = JSON.parse(pkgJson).devDependencies
  t.deepEqual(devDependencies, { '@rstacruz/tap-spec': '^4.1.1' }, 'tap-spec has been added to devDependencies')
})

test('multiple save to package.json with `exact` versions (@rstacruz/tap-spec & rimraf@2.5.1) (in sorted order)', async function (t) {
  const project = prepare(t)
  await installPkgs(['rimraf@2.5.1', '@rstacruz/tap-spec@latest'], testDefaults({ save: true, saveExact: true }))

  const tapSpec = project.requireModule('@rstacruz/tap-spec')
  t.ok(typeof tapSpec === 'function', 'tapSpec() is available')

  const rimraf = project.requireModule('rimraf')
  t.ok(typeof rimraf === 'function', 'rimraf() is available')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  const expectedDeps = {
    '@rstacruz/tap-spec': '4.1.1',
    rimraf: '2.5.1'
  }
  t.deepEqual(dependencies, expectedDeps, 'tap-spec and rimraf have been added to dependencies')
  t.deepEqual(Object.keys(dependencies), Object.keys(expectedDeps), 'tap-spec and rimraf have been added to dependencies in sorted order')
})

test('flattening symlinks (minimatch + balanced-match)', async function (t) {
  const project = prepare(t)
  await installPkgs(['minimatch@3.0.0'], testDefaults())
  await installPkgs(['balanced-match@^0.3.0'], testDefaults())

  let _ = await exists(path.join(process.cwd(), 'node_modules/.store/node_modules/balanced-match'))
  t.ok(!_, 'balanced-match is removed from store node_modules')

  await project.has('balanced-match')
})

test('production install (with --production flag)', async function (t) {
  const project = prepare(t, basicPackageJson)

  await install(testDefaults({ production: true }))

  const rimrafDir = fs.statSync(path.join(process.cwd(), 'node_modules', 'rimraf'))

  let tapStatErrCode: number = 0
  try {
    fs.statSync(path.join(process.cwd(), 'node_modules', '@rstacruz'))
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

  const rimrafDir = fs.statSync(path.join(process.cwd(), 'node_modules', 'rimraf'))

  let tapStatErrCode: number = 0
  try {
    fs.statSync(path.join(process.cwd(), 'node_modules', '@rstacruz'))
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
    }
  })

  const result = execPnpmSync('install', 'hello-world-js-bin')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'postinstall script was executed')

  t.end()
})

test('postinstall is executed after installation', t => {
  const project = prepare(t, {
    scripts: {
      postinstall: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install', 'is-negative')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'postinstall script was executed')

  t.end()
})

test('prepublish is not executed after installation with arguments', t => {
  const project = prepare(t, {
    scripts: {
      prepublish: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install', 'is-negative')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') === -1, 'prepublish script was not executed')

  t.end()
})

test('prepublish is executed after argumentless installation', t => {
  const project = prepare(t, {
    scripts: {
      prepublish: 'echo "Hello world!"'
    }
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'prepublish script was executed')

  t.end()
})

test('global installation', async function (t) {
  prepare(t)
  const opts = testDefaults({global: true})
  await installPkgs(['is-positive'], opts)

  const isPositive = require(path.join(opts.globalPath, 'node_modules', 'is-positive'))
  t.ok(typeof isPositive === 'function', 'isPositive() is available')
})

test('tarball local package', async function (t) {
  const project = prepare(t)
  await installPkgs([pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz')], testDefaults())

  const localPkg = project.requireModule('tar-pkg')

  t.equal(localPkg(), 'tar-pkg', 'tarPkg() is available')
})

test("don't fail when peer dependency is fetched from GitHub", t => {
  const project = prepare(t)
  return installPkgs(['test-pnpm-peer-deps'], testDefaults())
})

test('peer dependency is linked', async t => {
  const project = prepare(t)
  await installPkgs(['ajv@4.10.4', 'ajv-keywords@1.5.0'], testDefaults())

  t.ok(await exists(path.join('node_modules', '.localhost+4873', 'ajv-keywords', '1.5.0', 'node_modules', 'ajv')), 'peer dependency is linked')
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

  const latest = 'stable'

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', latest)

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true, tag: latest}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', latest)

  await install(testDefaults({depth: 1, tag: latest}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
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

  const pkg = project.requireModule('.localhost+4873/pkg-with-1-dep/100.0.0/node_modules/dep-of-pkg-with-1-dep/package.json')

  t.equal(pkg.version, '100.0.0', 'dependency specified in shrinkwrap.yaml is installed')
})

test('self-require should work', async function (t) {
  const project = prepare(t)

  await installPkgs(['uses-pkg-with-self-usage'], testDefaults())

  t.ok(project.requireModule('uses-pkg-with-self-usage'))
})
