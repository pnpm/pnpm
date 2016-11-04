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
import isExecutable from './support/isExecutable'
import testDefaults from './support/testDefaults'
import exists = require('exists-file')
import globalPath from './support/globalPath'
import {pathToLocalPkg, local} from './support/localPkg'

const isWindows = process.platform === 'win32'
const preserveSymlinks = semver.satisfies(process.version, '>=6.3.0')

if (!caw() && !isWindows) {
  process.env.VCR_MODE = 'cache'
}

test('small with dependencies (rimraf)', async function (t) {
  prepare()
  await installPkgs(['rimraf@2.5.1'], testDefaults())

  const rimraf = require(path.join(process.cwd(), 'node_modules', 'rimraf'))
  t.ok(typeof rimraf === 'function', 'rimraf() is available')
  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'rimraf'))
})

test('no dependencies (lodash)', async function (t) {
  prepare()
  await installPkgs(['lodash@4.0.0'], testDefaults())

  const _ = require(path.join(process.cwd(), 'node_modules', 'lodash'))
  t.ok(typeof _ === 'function', '_ is available')
  t.ok(typeof _.clone === 'function', '_.clone is available')
})

test('scoped modules without version spec (@rstacruz/tap-spec)', async function (t) {
  prepare()
  await installPkgs(['@rstacruz/tap-spec'], testDefaults())

  const _ = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
  t.ok(typeof _ === 'function', 'tap-spec is available')
})

test('scoped modules with versions (@rstacruz/tap-spec@4.1.1)', async function (t) {
  prepare()
  await installPkgs(['@rstacruz/tap-spec@4.1.1'], testDefaults())

  const _ = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
  t.ok(typeof _ === 'function', 'tap-spec is available')
})

test('scoped modules (@rstacruz/tap-spec@*)', async function (t) {
  prepare()
  await installPkgs(['@rstacruz/tap-spec@*'], testDefaults())

  const _ = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
  t.ok(typeof _ === 'function', 'tap-spec is available')
})

test('multiple scoped modules (@rstacruz/...)', async function (t) {
  prepare()
  await installPkgs(['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], testDefaults())

  const tapSpec = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
  t.ok(typeof tapSpec === 'function', 'tap-spec is available')
  const travisEncrypt = require(path.join(process.cwd(), 'node_modules', '@rstacruz/travis-encrypt'))
  t.ok(typeof travisEncrypt === 'function', 'travis-encrypt is available')
})

test('nested scoped modules (test-pnpm-issue219 -> @zkochan/test-pnpm-issue219)', async function (t) {
  prepare()
  await installPkgs(['test-pnpm-issue219@1.0.2'], testDefaults())

  const _ = require(path.join(process.cwd(), 'node_modules', 'test-pnpm-issue219'))
  t.ok(_ === 'test-pnpm-issue219,@zkochan/test-pnpm-issue219', 'nested scoped package is available')
})

test('scoped modules from a directory', async function (t) {
  prepare()
  await installPkgs([local('local-scoped-pkg')], testDefaults())

  const localPkg = require(
    path.join(process.cwd(), 'node_modules', '@scope', 'local-scoped-pkg'))

  t.equal(localPkg(), '@scope/local-scoped-pkg', 'localScopedPkg() is available')
})

test('skip failing optional dependencies', async function (t) {
  prepare()
  await installPkgs(['pkg-with-failing-optional-dependency@1.0.1'], testDefaults())

  const isNegative = require(path.join(process.cwd(), 'node_modules', 'pkg-with-failing-optional-dependency'))
  t.ok(isNegative(-1), 'package with failed optional dependency has the dependencies installed correctly')
})

test('idempotency (rimraf)', async function (t) {
  prepare()
  await installPkgs(['rimraf@2.5.1'], testDefaults())
  await installPkgs(['rimraf@2.5.1'], testDefaults())

  const rimraf = require(path.join(process.cwd(), 'node_modules', 'rimraf'))
  t.ok(typeof rimraf === 'function', 'rimraf is available')
})

test('overwriting (magic-hook@2.0.0 and @0.1.0)', async function (t) {
  prepare()
  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const flattenPathInStore = path.join(process.cwd(), 'node_modules/.store/flatten@1.0.2')
  let flattenExists = await exists(flattenPathInStore)
  t.ok(flattenExists, 'flatten@1.0.2 is in the store')

  await installPkgs(['magic-hook@0.1.0'], testDefaults())

  flattenExists = await exists(flattenPathInStore)
  t.ok(!flattenExists, 'dependency of magic-hook@2.0.0 is removed')

  const _ = require(path.join(process.cwd(), 'node_modules', 'magic-hook', 'package.json'))
  t.ok(_.version === '0.1.0', 'magic-hook is 0.1.0')
})

test('overwriting (is-positive@3.0.0 with is-positive@latest)', async function (t) {
  prepare()
  await installPkgs(['is-positive@3.0.0'], testDefaults({save: true}))

  let _ = await exists(path.join(process.cwd(), 'node_modules/.store/is-positive@3.0.0'))
  t.ok(_, 'magic-hook@3.0.0 exists')

  await installPkgs(['is-positive@latest'], testDefaults({save: true}))

  _ = await exists(path.join(process.cwd(), 'node_modules/.store/is-positive@3.1.0'))
  t.ok(_, 'magic-hook@3.1.0 exists after installing the latest')
})

test('forcing', async function (t) {
  prepare()
  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPath = path.join(process.cwd(), 'node_modules/.store/magic-hook@2.0.0/_/dist')
  await rimraf(distPath)

  await installPkgs(['magic-hook@2.0.0'], testDefaults({force: true}))

  const distPathExists = await exists(distPath)
  t.ok(distPathExists, 'magic-hook@2.0.0 dist folder reinstalled')
})

test('no forcing', async function (t) {
  prepare()
  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPath = path.join(process.cwd(), 'node_modules/.store/magic-hook@2.0.0/_/dist')
  await rimraf(distPath)

  await installPkgs(['magic-hook@2.0.0'], testDefaults())

  const distPathExists = await exists(distPath)
  t.ok(!distPathExists, 'magic-hook@2.0.0 dist folder not reinstalled')
})

test('circular deps', async function (t) {
  prepare()
  await installPkgs(['circular-deps-1-of-2'], testDefaults())

  const dep = require(path.join(process.cwd(), 'node_modules/circular-deps-1-of-2/mirror'))

  t.equal(dep(), 'circular-deps-1-of-2', 'circular dependencies can access each other')
})

test('big with dependencies and circular deps (babel-preset-2015)', async function (t) {
  prepare()
  await installPkgs(['babel-preset-es2015@6.3.13'], testDefaults())

  const b = require(path.join(process.cwd(), 'node_modules', 'babel-preset-es2015'))
  t.ok(typeof b === 'object', 'babel-preset-es2015 is available')
})

test('bundleDependencies (fsevents@1.0.6)', async function (t) {
  if (isWindows) {
    t.skip("fsevents can't be installed on Windows")
    return
  }

  prepare()
  await installPkgs(['fsevents@1.0.6'], testDefaults())

  isExecutable(t, path.join(process.cwd(), 'node_modules', 'fsevents', 'node_modules', '.bin', 'mkdirp'))
})

test('compiled modules (ursa@0.9.1)', async function (t) {
  if (!isCI || isWindows) {
    t.skip('only ran on CI')
    return t.end()
  }

  prepare()
  await installPkgs(['ursa@0.9.1'], testDefaults())

  const ursa = require(path.join(process.cwd(), 'node_modules', 'ursa'))
  t.ok(typeof ursa === 'object', 'ursa() is available')
})

test('tarballs (is-array-1.0.1.tgz)', async function (t) {
  prepare()
  await installPkgs(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], testDefaults())

  const isArray = require(
    path.join(process.cwd(), 'node_modules', 'is-array'))

  t.ok(isArray, 'isArray() is available')

  const stat = fs.statSync(
    path.join(process.cwd(), 'node_modules/.store',
      'is-array-1.0.1#a83102a9c117983e6ff4d85311fb322231abe3d6'))
  t.ok(stat.isDirectory(), 'stored in the proper location')
})

test('tarballs from GitHub (is-negative)', async function (t) {
  prepare()
  await installPkgs(['is-negative@https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz'], testDefaults())

  const isNegative = require(
    path.join(process.cwd(), 'node_modules', 'is-negative'))

  t.ok(isNegative, 'isNegative() is available')
})

test('local file', async function (t) {
  prepare()
  await installPkgs([local('local-pkg')], testDefaults())

  const localPkg = require(
    path.join(process.cwd(), 'node_modules', 'local-pkg'))

  t.ok(localPkg, 'localPkg() is available')
})

test('nested local dependency of a local dependency', async function (t) {
  prepare()
  await installPkgs([local('pkg-with-local-dep')], testDefaults())

  const pkgWithLocalDep = require(
    path.join(process.cwd(), 'node_modules', 'pkg-with-local-dep'))

  t.ok(pkgWithLocalDep, 'pkgWithLocalDep() is available')

  t.equal(pkgWithLocalDep(), 'local-pkg', 'pkgWithLocalDep() returns data from local-pkg')
})

test('from a github repo', async function (t) {
  prepare()
  await installPkgs(['kevva/is-negative'], testDefaults())

  const localPkg = require(
    path.join(process.cwd(), 'node_modules', 'is-negative'))

  t.ok(localPkg, 'isNegative() is available')
})

test('from a git repo', async function (t) {
  if (isCI) {
    t.skip('not testing the SSH GIT access via CI')
    return t.end()
  }
  prepare()
  await installPkgs(['git+ssh://git@github.com/kevva/is-negative.git'], testDefaults())

  const localPkg = require(
    path.join(process.cwd(), 'node_modules', 'is-negative'))

  t.ok(localPkg, 'isNegative() is available')
})

test('shrinkwrap compatibility', async function (t) {
  prepare({ dependencies: { rimraf: '*' } })

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
  prepare()
  await installPkgs([local('pre-and-postinstall-scripts-example')], testDefaults())

  const generatedByPreinstall = require(path.join(process.cwd(), 'node_modules', 'pre-and-postinstall-scripts-example/generated-by-preinstall'))
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = require(path.join(process.cwd(), 'node_modules', 'pre-and-postinstall-scripts-example/generated-by-postinstall'))
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
})

test('run install scripts', async function (t) {
  prepare()
  await installPkgs([local('install-script-example')], testDefaults())

  const generatedByInstall = require(path.join(process.cwd(), 'node_modules', 'install-script-example/generated-by-install'))
  t.ok(typeof generatedByInstall === 'function', 'generatedByInstall() is available')
})

test('save to package.json (rimraf@2.5.1)', async function (t) {
  prepare()
  await installPkgs(['rimraf@2.5.1'], testDefaults({ save: true }))

  const rimraf = require(path.join(process.cwd(), 'node_modules', 'rimraf'))
  t.ok(typeof rimraf === 'function', 'rimraf() is available')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const dependencies = JSON.parse(pkgJson).dependencies
  t.deepEqual(dependencies, {rimraf: '^2.5.1'}, 'rimraf has been added to dependencies')
})

test('saveDev scoped module to package.json (@rstacruz/tap-spec)', async function (t) {
  prepare()
  await installPkgs(['@rstacruz/tap-spec'], testDefaults({ saveDev: true }))

  const tapSpec = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
  t.ok(typeof tapSpec === 'function', 'tapSpec() is available')

  const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
  const devDependencies = JSON.parse(pkgJson).devDependencies
  t.deepEqual(devDependencies, { '@rstacruz/tap-spec': '^4.1.1' }, 'tap-spec has been added to devDependencies')
})

test('multiple save to package.json with `exact` versions (@rstacruz/tap-spec & rimraf@2.5.1) (in sorted order)', async function (t) {
  prepare()
  await installPkgs(['rimraf@2.5.1', '@rstacruz/tap-spec@latest'], testDefaults({ save: true, saveExact: true }))

  const tapSpec = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
  t.ok(typeof tapSpec === 'function', 'tapSpec() is available')

  const rimraf = require(path.join(process.cwd(), 'node_modules', 'rimraf'))
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

test('flattening symlinks (minimatch@3.0.0)', async function (t) {
  if (preserveSymlinks) {
    t.skip('this is required only for Node.JS < 6.3.0')
    return
  }
  prepare()
  await installPkgs(['minimatch@3.0.0'], testDefaults())

  const stat = fs.lstatSync(path.join(process.cwd(), 'node_modules/.store/node_modules/balanced-match'))
  t.ok(stat.isSymbolicLink(), 'balanced-match is linked into store node_modules')

  const _ = await exists(path.join(process.cwd(), 'node_modules', 'balanced-match'))
  t.ok(!_, 'balanced-match is not linked into main node_modules')
})

test('flattening symlinks (minimatch + balanced-match)', async function (t) {
  prepare()
  await installPkgs(['minimatch@3.0.0'], testDefaults())
  await installPkgs(['balanced-match@^0.3.0'], testDefaults())

  let _ = await exists(path.join(process.cwd(), 'node_modules/.store/node_modules/balanced-match'))
  t.ok(!_, 'balanced-match is removed from store node_modules')

  _ = await exists(path.join(process.cwd(), 'node_modules', 'balanced-match'))
  t.ok(_, 'balanced-match now in main node_modules')
})

test('production install (with --production flag)', async function (t) {
  prepare(basicPackageJson)

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
  prepare(basicPackageJson)

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

test('fail when trying to install into the same store simultaneously', t => {
  prepare()
  return Promise.all([
    installPkgs([local('pkg-that-installs-slowly')], testDefaults()),
    wait(500) // to be sure that lock was created
      .then(_ => installPkgs(['rimraf@2.5.1'], testDefaults()))
      .then(_ => t.fail('the store should have been locked'))
      .catch(err => t.ok(err, 'store is locked'))
  ])
})

test('fail when trying to install and uninstall from the same store simultaneously', t => {
  prepare()
  return Promise.all([
    installPkgs([local('pkg-that-installs-slowly')], testDefaults()),
    wait(500) // to be sure that lock was created
      .then(_ => uninstall(['rimraf@2.5.1'], testDefaults()))
      .then(_ => t.fail('the store should have been locked'))
      .catch(err => t.ok(err, 'store is locked'))
  ])
})

test('packages should find the plugins they use when symlinks are preserved', async function (t) {
  if (!preserveSymlinks) {
    t.skip('this test only for NodeJS with --preserve-symlinks support')
    return
  }
  prepare({
    scripts: {
      test: 'pkg-that-uses-plugins'
    }
  })
  await installPkgs([local('pkg-that-uses-plugins'), local('plugin-example')], testDefaults({ save: true }))
  const result = spawnSync('npm', ['test'])
  t.ok(result.stdout.toString().indexOf('My plugin is plugin-example') !== -1, 'package executable have found its plugin')
  t.equal(result.status, 0, 'executable exited with success')
})

test('run js bin file', async function (t) {
  prepare({
    scripts: {
      test: 'hello-world-js-bin'
    }
  })
  await installPkgs([local('hello-world-js-bin')], testDefaults({ save: true }))

  const result = spawnSync('npm', ['test'])
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'package executable printed its message')
  t.equal(result.status, 0, 'executable exited with success')
})

const pnpmBin = path.join(__dirname, '../src/bin/pnpm.ts')

test('bin files are found by lifecycle scripts', t => {
  prepare({
    scripts: {
      postinstall: 'hello-world-js-bin'
    }
  })

  const result = spawnSync('ts-node', [pnpmBin, 'install', local('hello-world-js-bin')])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'postinstall script was executed')

  t.end()
})

test('installation via the CLI', t => {
  prepare()
  const result = spawnSync('ts-node', [pnpmBin, 'install', 'rimraf@2.5.1'])

  console.log(result.stderr.toString())
  t.equal(result.status, 0, 'install successful')

  const rimraf = require(path.join(process.cwd(), 'node_modules', 'rimraf'))
  t.ok(typeof rimraf === 'function', 'rimraf() is available')

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'rimraf'))

  t.end()
})

test('pass through to npm CLI for commands that are not supported by npm', t => {
  const result = spawnSync('ts-node', [pnpmBin, 'config', 'get', 'user-agent'])

  t.equal(result.status, 0, 'command was successfull')
  t.ok(result.stdout.toString().indexOf('npm/') !== -1, 'command returned correct result')

  t.end()
})

test('postinstall is executed after installation', t => {
  prepare({
    scripts: {
      postinstall: 'echo "Hello world!"'
    }
  })

  const result = spawnSync('ts-node', [pnpmBin, 'install', 'is-negative'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'postinstall script was executed')

  t.end()
})

test('prepublish is not executed after installation with arguments', t => {
  prepare({
    scripts: {
      prepublish: 'echo "Hello world!"'
    }
  })

  const result = spawnSync('ts-node', [pnpmBin, 'install', 'is-negative'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') === -1, 'prepublish script was not executed')

  t.end()
})

test('prepublish is executed after argumentless installation', t => {
  prepare({
    scripts: {
      prepublish: 'echo "Hello world!"'
    }
  })

  const result = spawnSync('ts-node', [pnpmBin, 'install'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'prepublish script was executed')

  t.end()
})

test('global installation', async function (t) {
  await installPkgs(['is-positive'], testDefaults({globalPath, global: true}))

  const isPositive = require(path.join(globalPath, 'node_modules', 'is-positive'))
  t.ok(typeof isPositive === 'function', 'isPositive() is available')
})

test('tarball local package', async function (t) {
  prepare()
  await installPkgs([pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz')], testDefaults())

  const localPkg = require(path.join(process.cwd(), 'node_modules', 'tar-pkg'))

  t.equal(localPkg(), 'tar-pkg', 'tarPkg() is available')
})

test("don't fail when peer dependency is fetched from GitHub", t => {
  prepare()
  return installPkgs([local('test-pnpm-peer-deps')], testDefaults())
})

test('create a pnpm-debug.log file when the command fails', async function (t) {
  prepare()

  const result = spawnSync('ts-node', [pnpmBin, 'install', '@zkochan/i-do-not-exist'])

  t.equal(result.status, 1, 'install failed')

  t.ok(await exists('pnpm-debug.log'), 'log file created')

  t.end()
})

test('building native addons', async function (t) {
  prepare()

  await installPkgs(['runas@3.1.1'], testDefaults())

  t.ok(await exists('node_modules/.store/runas@3.1.1/_/build'), 'build folder created')
})

test('should update subdep on second install', async function (t) {
  prepare()

  const latest = 'stable'

  await addDistTag('dep-of-pkg-with-1-dep', '1.0.0', latest)

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true, tag: latest, cacheTTL: 0}))

  t.ok(await exists('node_modules/.store/dep-of-pkg-with-1-dep@1.0.0'), 'should install dep-of-pkg-with-1-dep@1.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '1.1.0', latest)

  await install(testDefaults({depth: 1, tag: latest, cacheTTL: 0}))

  t.ok(await exists('node_modules/.store/dep-of-pkg-with-1-dep@1.1.0'), 'should update to dep-of-pkg-with-1-dep@1.1.0')
})

test('should install flat tree', async function (t) {
  if (!preserveSymlinks) {
    t.skip('this test only for NodeJS with --preserve-symlinks support')
    return
  }

  prepare()
  await installPkgs(['rimraf@2.5.1'], testDefaults({flatTree: true}))

  isAvailable('balanced-match')
  isAvailable('rimraf')
  isAvailable('brace-expansion')
  isAvailable('concat-map')

  function isAvailable (depName: string) {
    const dep = require(path.join(process.cwd(), 'node_modules', depName))
    t.ok(dep, `${depName} is available`)
  }
})

test('should throw error when trying to install flat tree on Node.js < 6.3.0', async function (t) {
  if (preserveSymlinks) {
    t.skip()
    return
  }

  prepare()

  try {
    await installPkgs(['rimraf@2.5.1'], testDefaults({flatTree: true}))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, '`--preserve-symlinks` and so `--flat-tree` are not supported on your system, make sure you are running on Node ≽ 6.3.0')
  }
})

test('should throw error when trying to install with a different tree type using a dedicated store', async function(t) {
  if (!preserveSymlinks) {
    t.skip('flat trees are supported only on Node.js with --preserve-symlinks support')
    return
  }

  prepare()

  await installPkgs(['rimraf@2.5.1'], testDefaults({flatTree: false}))

  try {
    await installPkgs(['is-negative'], testDefaults({flatTree: true}))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'INCONSISTENT_TREE_TYPE', 'failed with correct error code')
  }
})

test('should throw error when trying to install with a different tree type using a global store', async function(t) {
  if (!preserveSymlinks) {
    t.skip('flat trees are supported only on Node.js with --preserve-symlinks support')
    return
  }

  prepare()

  await installPkgs(['rimraf@2.5.1'], testDefaults({flatTree: false, global: true}))

  try {
    await installPkgs(['is-negative'], testDefaults({flatTree: true, global: true}))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'INCONSISTENT_TREE_TYPE', 'failed with correct error code')
  }
})

test('should throw error when trying to install using a different store then the previous one', async function(t) {
  prepare()

  await installPkgs(['rimraf@2.5.1'], testDefaults({storePath: 'node_modules/.store1'}))

  try {
    await installPkgs(['is-negative'], testDefaults({storePath: 'node_modules/.store2'}))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'ALIEN_STORE', 'failed with correct error code')
  }
})

test('should not throw error if using a different store after all the packages were uninstalled', async function(t) {
  // TODO: implement
})

test('should reinstall package to the store if it is not in the store.yml', async function (t) {
  prepare()

  try {
    await installPkgs(['is-positive@3.1.0', 'this-pkg-does-not-exist-3f49f4'], testDefaults())
    t.fail('installation should have failed')
  } catch (err) {}

  await rimraf(path.join(process.cwd(), 'node_modules/.store/is-positive@3.1.0/_/index.js'))

  await installPkgs(['is-positive@3.1.0'], testDefaults())

  t.ok(await exists(path.join(process.cwd(), 'node_modules/.store/is-positive@3.1.0/_/index.js')))
})
