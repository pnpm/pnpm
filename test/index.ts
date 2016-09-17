import 'sepia'
import test = require('tape')
import {Test} from 'tape'
import path = require('path')
import fs = require('fs')
import caw = require('caw')
import isexe = require('isexe')
import semver = require('semver')
import crossSpawn = require('cross-spawn')
const spawnSync = crossSpawn.sync
import thenify = require('thenify')
import ncpCB = require('ncp')
const ncp = thenify(ncpCB.ncp)
import mkdirp = require('mkdirp')
import prepare from './support/prepare'
import requireJson from '../src/fs/requireJson'
const basicPackageJson = requireJson(path.join(__dirname, './support/simple-package.json'))
import install from '../src/cmd/install'
import uninstall from '../src/cmd/uninstall'
import link from '../src/cmd/link'
import * as pnpm from '../src'

const isWindows = process.platform === 'win32'
const preserveSymlinks = semver.satisfies(process.version, '>=6.3.0')
const globalPath = path.join(process.cwd(), '.tmp', 'global')

if (!caw() && !isWindows) {
  process.env.VCR_MODE = 'cache'
}

function isExecutable (t: Test, filePath: string) {
  if (!isWindows && !preserveSymlinks) {
    const lstat = fs.lstatSync(filePath)
    t.ok(lstat.isSymbolicLink(), filePath + ' symlink is available')

    const stat = fs.statSync(filePath)
    t.equal(stat.mode, parseInt('100755', 8), filePath + ' is executable')
    t.ok(stat.isFile(), filePath + ' refers to a file')
    return
  }
  t.ok(isexe(filePath), filePath + ' is executable')
}

test('API', t => {
  t.equal(typeof pnpm.install, 'function', 'exports install()')
  t.equal(typeof pnpm.install, 'function', 'exports installPkgDeps()')
  t.equal(typeof pnpm.uninstall, 'function', 'exports uninstall()')
  t.equal(typeof pnpm.linkFromGlobal, 'function', 'exports linkFromGlobal()')
  t.equal(typeof pnpm.linkFromRelative, 'function', 'exports linkFromRelative()')
  t.equal(typeof pnpm.linkToGlobal, 'function', 'exports linkToGlobal()')
  t.end()
})

test('small with dependencies (rimraf)', t => {
  prepare()
  install(['rimraf@2.5.1'], { quiet: true })
  .then(() => {
    const rimraf = require(path.join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf() is available')

    isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'rimraf'))

    t.end()
  })
  .catch(t.end)
})

test('no dependencies (lodash)', t => {
  prepare()
  install(['lodash@4.0.0'], { quiet: true })
  .then(() => {
    const _ = require(path.join(process.cwd(), 'node_modules', 'lodash'))
    t.ok(typeof _ === 'function', '_ is available')
    t.ok(typeof _.clone === 'function', '_.clone is available')
    t.end()
  })
  .catch(t.end)
})

test('scoped modules without version spec (@rstacruz/tap-spec)', t => {
  prepare()
  install(['@rstacruz/tap-spec'], { quiet: true })
  .then(() => {
    const _ = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  })
  .catch(t.end)
})

test('scoped modules with versions (@rstacruz/tap-spec@4.1.1)', t => {
  prepare()
  install(['@rstacruz/tap-spec@4.1.1'], { quiet: true })
  .then(() => {
    const _ = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  })
  .catch(t.end)
})

test('scoped modules (@rstacruz/tap-spec@*)', t => {
  prepare()
  install(['@rstacruz/tap-spec@*'], { quiet: true })
  .then(() => {
    const _ = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  })
  .catch(t.end)
})

test('multiple scoped modules (@rstacruz/...)', t => {
  prepare()
  install(['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], { quiet: true })
  .then(() => {
    const tapSpec = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof tapSpec === 'function', 'tap-spec is available')
    const travisEncrypt = require(path.join(process.cwd(), 'node_modules', '@rstacruz/travis-encrypt'))
    t.ok(typeof travisEncrypt === 'function', 'travis-encrypt is available')
    t.end()
  })
  .catch(t.end)
})

test('nested scoped modules (test-pnpm-issue219 -> @zkochan/test-pnpm-issue219)', t => {
  prepare()
  install(['test-pnpm-issue219@1.0.2'], { quiet: true })
  .then(() => {
    const _ = require(path.join(process.cwd(), 'node_modules', 'test-pnpm-issue219'))
    t.ok(_ === 'test-pnpm-issue219,@zkochan/test-pnpm-issue219', 'nested scoped package is available')
    t.end()
  })
  .catch(t.end)
})

test('scoped modules from a directory', t => {
  prepare()
  install([local('local-scoped-pkg')], { quiet: true })
  .then(() => {
    const localPkg = require(
      path.join(process.cwd(), 'node_modules', '@scope', 'local-scoped-pkg'))

    t.equal(localPkg(), '@scope/local-scoped-pkg', 'localScopedPkg() is available')

    t.end()
  })
  .catch(t.end)
})

test('skip failing optional dependencies', t => {
  prepare()
  install(['pkg-with-failing-optional-dependency@1.0.1'], { quiet: true })
  .then(() => {
    const isNegative = require(path.join(process.cwd(), 'node_modules', 'pkg-with-failing-optional-dependency'))
    t.ok(isNegative(-1), 'package with failed optional dependency has the dependencies installed correctly')
    t.end()
  })
  .catch(t.end)
})

test('idempotency (rimraf)', t => {
  prepare()
  install(['rimraf@2.5.1'], { quiet: true })
  .then(() => install([ 'rimraf@2.5.1' ], { quiet: true }))
  .then(() => {
    const rimraf = require(path.join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf is available')
    t.end()
  })
  .catch(t.end)
})

test('overwriting (lodash@3.10.1 and @4.0.0)', t => {
  prepare()
  install(['lodash@3.10.1'], { quiet: true })
  .then(() => install([ 'lodash@4.0.0' ], { quiet: true }))
  .then(() => {
    const _ = require(path.join(process.cwd(), 'node_modules', 'lodash', 'package.json'))
    t.ok(_.version === '4.0.0', 'lodash is 4.0.0')
    t.end()
  })
  .catch(t.end)
})

test('big with dependencies and circular deps (babel-preset-2015)', t => {
  prepare()
  install(['babel-preset-es2015@6.3.13'], { quiet: true })
  .then(() => {
    const b = require(path.join(process.cwd(), 'node_modules', 'babel-preset-es2015'))
    t.ok(typeof b === 'object', 'babel-preset-es2015 is available')
    t.end()
  })
  .catch(t.end)
})

// NOTE: fsevents can't be installed on Windows
if (!isWindows) {
  test('bundleDependencies (fsevents@1.0.6)', t => {
    prepare()
    install(['fsevents@1.0.6'], { quiet: true })
    .then(() => {
      isExecutable(t, path.join(process.cwd(), 'node_modules', 'fsevents', 'node_modules', '.bin', 'mkdirp'))
      t.end()
    })
    .catch(t.end)
  })
}

test('compiled modules (ursa@0.9.1)', t => {
  if (!process.env.CI || isWindows) {
    t.skip('only ran on CI')
    return t.end()
  }

  prepare()
  install(['ursa@0.9.1'], { quiet: false })
  .then(() => {
    const ursa = require(path.join(process.cwd(), 'node_modules', 'ursa'))
    t.ok(typeof ursa === 'object', 'ursa() is available')
    t.end()
  })
  .catch(t.end)
})

test('tarballs (is-array-1.0.1.tgz)', t => {
  prepare()
  install(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], { quiet: true })
  .then(() => {
    const isArray = require(
      path.join(process.cwd(), 'node_modules', 'is-array'))

    t.ok(isArray, 'isArray() is available')

    const stat = fs.statSync(
      path.join(process.cwd(), 'node_modules', '.store',
        'is-array-1.0.1#a83102a9c117983e6ff4d85311fb322231abe3d6'))
    t.ok(stat.isDirectory(), 'stored in the proper location')
    t.end()
  })
  .catch(t.end)
})

test('tarballs from GitHub (is-negative)', t => {
  prepare()
  install(['is-negative@https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz'], { quiet: true })
  .then(() => {
    const isNegative = require(
      path.join(process.cwd(), 'node_modules', 'is-negative'))

    t.ok(isNegative, 'isNegative() is available')

    t.end()
  })
  .catch(t.end)
})

test('local file', t => {
  prepare()
  install([local('local-pkg')], { quiet: true })
  .then(() => {
    const localPkg = require(
      path.join(process.cwd(), 'node_modules', 'local-pkg'))

    t.ok(localPkg, 'localPkg() is available')

    t.end()
  })
  .catch(t.end)
})

test('nested local dependency of a local dependency', t => {
  prepare()
  install([local('pkg-with-local-dep')], { quiet: true })
  .then(() => {
    const pkgWithLocalDep = require(
      path.join(process.cwd(), 'node_modules', 'pkg-with-local-dep'))

    t.ok(pkgWithLocalDep, 'pkgWithLocalDep() is available')

    t.equal(pkgWithLocalDep(), 'local-pkg', 'pkgWithLocalDep() returns data from local-pkg')

    t.end()
  })
  .catch(t.end)
})

test('link local package if link-local = true', t => {
  prepare()
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgDirName = linkedPkgName + Math.random().toString()
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgDirName)
  ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
    .then(() => install([`file:../${linkedPkgDirName}`], { quiet: true, linkLocal: true }))
    .then(() => {
      isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))

      t.end()
    })
    .catch(t.end)
})

test('from a github repo', t => {
  prepare()
  install(['kevva/is-negative'], { quiet: true })
  .then(() => {
    const localPkg = require(
      path.join(process.cwd(), 'node_modules', 'is-negative'))

    t.ok(localPkg, 'isNegative() is available')

    t.end()
  })
  .catch(t.end)
})

test('shrinkwrap compatibility', t => {
  prepare({ dependencies: { rimraf: '*' } })

  install(['rimraf@2.5.1'], { quiet: true })
  .then(() => {
    return new Promise((resolve, reject) => {
      const proc = crossSpawn.spawn('npm', ['shrinkwrap'])

      proc.on('error', reject)

      proc.on('close', (code: number) => {
        if (code > 0) return reject(new Error('Exit code ' + code))
        const wrap = JSON.parse(fs.readFileSync('npm-shrinkwrap.json', 'utf-8'))
        t.ok(wrap.dependencies.rimraf.version === '2.5.1',
          'npm shrinkwrap is successful')
        t.end()
      })
    })
  })
  .catch(t.end)
})

test('run pre/postinstall scripts', t => {
  prepare()
  install([local('pre-and-postinstall-scripts-example')], { quiet: true })
  .then(() => {
    const generatedByPreinstall = require(path.join(process.cwd(), 'node_modules', 'pre-and-postinstall-scripts-example/generated-by-preinstall'))
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = require(path.join(process.cwd(), 'node_modules', 'pre-and-postinstall-scripts-example/generated-by-postinstall'))
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')

    t.end()
  })
  .catch(t.end)
})

test('run install scripts', t => {
  prepare()
  install([local('install-script-example')], { quiet: true })
  .then(() => {
    const generatedByInstall = require(path.join(process.cwd(), 'node_modules', 'install-script-example/generated-by-install'))
    t.ok(typeof generatedByInstall === 'function', 'generatedByInstall() is available')

    t.end()
  })
  .catch(t.end)
})

test('save to package.json (rimraf@2.5.1)', t => {
  prepare()
  install(['rimraf@2.5.1'], { quiet: true, save: true })
  .then(() => {
    const rimraf = require(path.join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf() is available')

    const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
    const dependencies = JSON.parse(pkgJson).dependencies
    t.deepEqual(dependencies, {rimraf: '^2.5.1'}, 'rimraf has been added to dependencies')

    t.end()
  })
  .catch(t.end)
})

test('saveDev scoped module to package.json (@rstacruz/tap-spec)', t => {
  prepare()
  install(['@rstacruz/tap-spec'], { quiet: true, saveDev: true })
  .then(() => {
    const tapSpec = require(path.join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof tapSpec === 'function', 'tapSpec() is available')

    const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
    const devDependencies = JSON.parse(pkgJson).devDependencies
    t.deepEqual(devDependencies, { '@rstacruz/tap-spec': '^4.1.1' }, 'tap-spec has been added to devDependencies')

    t.end()
  })
  .catch(t.end)
})

test('multiple save to package.json with `exact` versions (@rstacruz/tap-spec & rimraf@2.5.1) (in sorted order)', t => {
  prepare()
  install(['rimraf@2.5.1', '@rstacruz/tap-spec@latest'], { quiet: true, save: true, saveExact: true })
  .then(() => {
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

    t.end()
  })
  .catch(t.end)
})

test('flattening symlinks (minimatch@3.0.0)', t => {
  prepare()
  install(['minimatch@3.0.0'], { quiet: true })
  .then(() => {
    const stat = fs.lstatSync(path.join(process.cwd(), 'node_modules', '.store', 'node_modules', 'balanced-match'))
    t.ok(stat.isSymbolicLink(), 'balanced-match is linked into store node_modules')

    const _ = exists(path.join(process.cwd(), 'node_modules', 'balanced-match'))
    t.ok(!_, 'balanced-match is not linked into main node_modules')
    t.end()
  })
  .catch(t.end)
})

test('flattening symlinks (minimatch + balanced-match)', t => {
  prepare()
  install(['minimatch@3.0.0'], { quiet: true })
  .then(() => install(['balanced-match@^0.3.0'], { quiet: true }))
  .then(() => {
    let _ = exists(path.join(process.cwd(), 'node_modules', '.store', 'node_modules', 'balanced-match'))
    t.ok(!_, 'balanced-match is removed from store node_modules')

    _ = exists(path.join(process.cwd(), 'node_modules', 'balanced-match'))
    t.ok(_, 'balanced-match now in main node_modules')
    t.end()
  })
  .catch(t.end)
})

test('production install (with --production flag)', t => {
  prepare(basicPackageJson)

  return install([], { quiet: true, production: true })
    .then(() => {
      const rimrafDir = fs.statSync(path.join(process.cwd(), 'node_modules', 'rimraf'))

      let tapStatErrCode: number = 0
      try {
        fs.statSync(path.join(process.cwd(), 'node_modules', '@rstacruz'))
      } catch (err) { tapStatErrCode = err.code }

      t.ok(rimrafDir.isSymbolicLink, 'rimraf exists')
      t.is(tapStatErrCode, 'ENOENT', 'tap-spec does not exist')

      t.end()
    })
    .catch(t.end)
})

test('production install (with production NODE_ENV)', t => {
  const originalNodeEnv = process.env.NODE_ENV
  process.env.NODE_ENV = 'production'
  prepare(basicPackageJson)

  return install([], { quiet: true })
    .then(() => {
      // reset NODE_ENV
      process.env.NODE_ENV = originalNodeEnv

      const rimrafDir = fs.statSync(path.join(process.cwd(), 'node_modules', 'rimraf'))

      let tapStatErrCode: number = 0
      try {
        fs.statSync(path.join(process.cwd(), 'node_modules', '@rstacruz'))
      } catch (err) { tapStatErrCode = err.code }

      t.ok(rimrafDir.isSymbolicLink, 'rimraf exists')
      t.is(tapStatErrCode, 'ENOENT', 'tap-spec does not exist')

      t.end()
    })
    .catch(t.end)
})

test('uninstall package with no dependencies', t => {
  prepare()
  install(['is-negative@2.1.0'], { quiet: true, save: true })
  .then(_ => uninstall(['is-negative'], { save: true }))
  .then(() => {
    let stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'is-negative@2.1.0'))
    t.ok(!stat, 'is-negative is removed from store')

    stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'is-negative'))
    t.ok(!stat, 'is-negative is removed from node_modules')

    const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
    const dependencies = JSON.parse(pkgJson).dependencies
    const expectedDeps = {}
    t.deepEqual(dependencies, expectedDeps, 'is-negative has been removed from dependencies')

    t.end()
  })
  .catch(t.end)
})

test('uninstall package with dependencies and do not touch other deps', t => {
  prepare()
  install(['is-negative@2.1.0', 'camelcase-keys@3.0.0'], { quiet: true, save: true })
  .then(_ => uninstall(['camelcase-keys'], { save: true }))
  .then(() => {
    let stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase-keys@2.1.0'))
    t.ok(!stat, 'camelcase-keys is removed from store')

    stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'camelcase-keys'))
    t.ok(!stat, 'camelcase-keys is removed from node_modules')

    stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase@3.0.0'))
    t.ok(!stat, 'camelcase is removed from store')

    stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'camelcase'))
    t.ok(!stat, 'camelcase is removed from node_modules')

    stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'map-obj@1.0.1'))
    t.ok(!stat, 'map-obj is removed from store')

    stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'map-obj'))
    t.ok(!stat, 'map-obj is removed from node_modules')

    stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'is-negative@2.1.0'))
    t.ok(stat, 'is-negative is not removed from store')

    stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'is-negative'))
    t.ok(stat, 'is-negative is not removed from node_modules')

    const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
    const dependencies = JSON.parse(pkgJson).dependencies
    const expectedDeps = {
      'is-negative': '^2.1.0'
    }
    t.deepEqual(dependencies, expectedDeps, 'camelcase-keys has been removed from dependencies')

    t.end()
  })
  .catch(t.end)
})

test('uninstall package with its bin files', t => {
  prepare()
  install(['sh-hello-world@1.0.0'], { quiet: true, save: true })
  .then(_ => uninstall(['sh-hello-world'], { save: true }))
  .then(() => {
    // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
    let stat = existsSymlink(path.join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
    t.ok(!stat, 'sh-hello-world is removed from .bin')

    stat = exists(path.join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
    t.ok(!stat, 'sh-hello-world is removed from .bin')

    t.end()
  })
  .catch(t.end)
})

test('keep dependencies used by others', t => {
  prepare()
  install(['hastscript@3.0.0', 'camelcase-keys@3.0.0'], { quiet: true, save: true })
  .then(_ => uninstall(['camelcase-keys'], { save: true }))
  .then(() => {
    let stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase-keys@2.1.0'))
    t.ok(!stat, 'camelcase-keys is removed from store')

    stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'camelcase-keys'))
    t.ok(!stat, 'camelcase-keys is removed from node_modules')

    stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'camelcase@3.0.0'))
    t.ok(stat, 'camelcase is not removed from store')

    stat = exists(path.join(process.cwd(), 'node_modules', '.store', 'map-obj@1.0.1'))
    t.ok(!stat, 'map-obj is removed from store')

    stat = existsSymlink(path.join(process.cwd(), 'node_modules', 'map-obj'))
    t.ok(!stat, 'map-obj is removed from node_modules')

    const pkgJson = fs.readFileSync(path.join(process.cwd(), 'package.json'), 'utf8')
    const dependencies = JSON.parse(pkgJson).dependencies
    const expectedDeps = {
      'hastscript': '^3.0.0'
    }
    t.deepEqual(dependencies, expectedDeps, 'camelcase-keys has been removed from dependencies')

    t.end()
  })
  .catch(t.end)
})

const wait = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

test('fail when trying to install into the same store simultaneously', t => {
  prepare()
  Promise.all([
    install([local('pkg-that-installs-slowly')], { quiet: true }),
    wait(500) // to be sure that lock was created
      .then(_ => install(['rimraf@2.5.1'], { quiet: true }))
      .then(_ => t.fail('the store should have been locked'))
      .catch(err => t.ok(err, 'store is locked'))
  ])
  .then(_ => t.end())
  .catch(t.end)
})

test('fail when trying to install and uninstall from the same store simultaneously', t => {
  prepare()
  Promise.all([
    install([local('pkg-that-installs-slowly')], { quiet: true }),
    wait(500) // to be sure that lock was created
      .then(_ => uninstall(['rimraf@2.5.1'], { quiet: true }))
      .then(_ => t.fail('the store should have been locked'))
      .catch(err => t.ok(err, 'store is locked'))
  ])
  .then(_ => t.end())
  .catch(t.end)
})

if (preserveSymlinks) {
  test('packages should find the plugins they use when symlinks are preserved', t => {
    prepare()
    install([local('pkg-that-uses-plugins'), local('plugin-example')], { quiet: true, save: true })
      .then(_ => {
        const result = spawnSync('pkg-that-uses-plugins', [], {
          env: extendPathWithLocalBin()
        })
        t.equal(result.stdout.toString(), 'plugin-example\n', 'package executable have found its plugin')
        t.equal(result.status, 0, 'executable exited with success')
        t.end()
      })
      .catch(t.end)
  })
}

test('run js bin file', t => {
  prepare()
  install([local('hello-world-js-bin')], { quiet: true, save: true })
    .then(_ => {
      const result = spawnSync('hello-world-js-bin', [], {
        env: extendPathWithLocalBin()
      })
      t.equal(result.stdout.toString(), 'Hello world!\n', 'package executable printed its message')
      t.equal(result.status, 0, 'executable exited with success')
      t.end()
    })
    .catch(t.end)
})

const pnpmBin = path.join(__dirname, '../src/bin/pnpm.ts')

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

test('prepublish is executed after installation', t => {
  prepare({
    scripts: {
      prepublish: 'echo "Hello world!"'
    }
  })

  const result = spawnSync('ts-node', [pnpmBin, 'install', 'is-negative'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'prepublish script was executed')

  t.end()
})

test('global installation', t => {
  install(['is-positive'], {quiet: true, globalPath, global: true})
    .then(_ => {
      const isPositive = require(path.join(globalPath, 'node_modules', 'is-positive'))
      t.ok(typeof isPositive === 'function', 'isPositive() is available')

      t.end()
    })
    .catch(t.end)
})

test('relative link', t => {
  prepare()
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgDirName = linkedPkgName + Math.random().toString()
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgDirName)
  ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
    .then(() => link([`../${linkedPkgDirName}`], { quiet: true }))
    .then(() => {
      isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))

      t.end()
    })
    .catch(t.end)
})

test('global link', t => {
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  mkdirp.sync(tmpDir)
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgName + Math.random().toString())
  ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
    .then(() => {
      process.chdir(linkedPkgPath)
      return link([], { globalPath, quiet: true })
    })
    .then(() => {
      prepare()
      return link([linkedPkgName], { globalPath, quiet: true })
    })
    .then(() => {
      isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))

      t.end()
    })
    .catch(t.end)
})

test('tarball local package', t => {
  prepare()
  install([pathToLocalPkg('tar-pkg/tar-pkg-1.0.0.tgz')], { quiet: true })
  .then(() => {
    const localPkg = require(
      path.join(process.cwd(), 'node_modules', 'tar-pkg'))

    t.equal(localPkg(), 'tar-pkg', 'tarPkg() is available')

    t.end()
  })
  .catch(t.end)
})

test("don't fail when peer dependency is fetched from GitHub", t => {
  prepare()
  install([local('test-pnpm-peer-deps')], { quiet: true })
    .then(() => t.end())
    .catch(t.end)
})

test('create a pnpm-debug.log file when the command fails', t => {
  prepare()

  const result = spawnSync('ts-node', [pnpmBin, 'install', '@zkochan/i-do-not-exist'])

  t.equal(result.status, 1, 'install failed')

  exists('pnpm-debug.log')

  t.end()
})

function extendPathWithLocalBin () {
  return {
    PATH: [
      path.join(process.cwd(), 'node_modules', '.bin'),
      process.env.PATH
    ].join(path.delimiter)
  }
}

function pathToLocalPkg (pkgName: string) {
  return path.join(__dirname, 'packages', pkgName)
}

function local (pkgName: string) {
  return `file:${pathToLocalPkg(pkgName)}`
}

function exists (path: string) {
  try {
    return fs.statSync(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return null
}

function existsSymlink (path: string) {
  try {
    return fs.lstatSync(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return null
}
