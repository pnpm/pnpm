var test = require('tape')
var join = require('path').join
var delimiter = require('path').delimiter
var fs = require('fs')
var caw = require('caw')
var isexe = require('isexe')
var semver = require('semver')
var spawnSync = require('cross-spawn').sync
var prepare = require('./support/prepare')
var basicPackageJson = require('./support/simple-package.json')
var install = require('../index').install
var uninstall = require('../index').uninstall

var isWindows = process.platform === 'win32'
var preserveSymlinks = semver.satisfies(process.version, '>=6.3.0')

if (!caw() && !isWindows) {
  require('./support/sepia')
}

var stat, _

function isExecutable (t, filePath) {
  if (!isWindows && !preserveSymlinks) {
    stat = fs.lstatSync(filePath)
    t.ok(stat.isSymbolicLink(), filePath + ' symlink is available')

    stat = fs.statSync(filePath)
    t.equal(stat.mode, parseInt('100755', 8), filePath + ' is executable')
    t.ok(stat.isFile(), filePath + ' refers to a file')
    return
  }
  t.ok(isexe(filePath), filePath + ' is executable')
}

test('small with dependencies (rimraf)', function (t) {
  prepare()
  install(['rimraf@2.5.1'], { quiet: true })
  .then(function () {
    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf() is available')

    isExecutable(t, join(process.cwd(), 'node_modules', '.bin', 'rimraf'))

    t.end()
  }, t.end)
})

test('no dependencies (lodash)', function (t) {
  prepare()
  install(['lodash@4.0.0'], { quiet: true })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', 'lodash'))
    t.ok(typeof _ === 'function', '_ is available')
    t.ok(typeof _.clone === 'function', '_.clone is available')
    t.end()
  }, t.end)
})

test('scoped modules without version spec (@rstacruz/tap-spec)', function (t) {
  prepare()
  install(['@rstacruz/tap-spec'], { quiet: true })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  }, t.end)
})

test('scoped modules with versions (@rstacruz/tap-spec@4.1.1)', function (t) {
  prepare()
  install(['@rstacruz/tap-spec@4.1.1'], { quiet: true })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  }, t.end)
})

test('scoped modules (@rstacruz/tap-spec@*)', function (t) {
  prepare()
  install(['@rstacruz/tap-spec@*'], { quiet: true })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  }, t.end)
})

test('multiple scoped modules (@rstacruz/...)', function (t) {
  prepare()
  install(['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], { quiet: true })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/travis-encrypt'))
    t.ok(typeof _ === 'function', 'travis-encrypt is available')
    t.end()
  }, t.end)
})

test('nested scoped modules (test-pnpm-issue219 -> @zkochan/test-pnpm-issue219)', function (t) {
  prepare()
  install(['test-pnpm-issue219@1.0.2'], { quiet: true })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', 'test-pnpm-issue219'))
    t.ok(_ === 'test-pnpm-issue219,@zkochan/test-pnpm-issue219', 'nested scoped package is available')
    t.end()
  }, t.end)
})

test('skip failing optional dependencies', function (t) {
  prepare()
  install(['pkg-with-failing-optional-dependency@1.0.1'], { quiet: true })
  .then(function () {
    var isNegative = require(join(process.cwd(), 'node_modules', 'pkg-with-failing-optional-dependency'))
    t.ok(isNegative(-1), 'package with failed optional dependency has the dependencies installed correctly')
    t.end()
  }, t.end)
})

test('idempotency (rimraf)', function (t) {
  prepare()
  install(['rimraf@2.5.1'], { quiet: true })
  .then(function () { return install([ 'rimraf@2.5.1' ], { quiet: true }) })
  .then(function () {
    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf is available')
    t.end()
  }, t.end)
})

test('overwriting (lodash@3.10.1 and @4.0.0)', function (t) {
  prepare()
  install(['lodash@3.10.1'], { quiet: true })
  .then(function () { return install([ 'lodash@4.0.0' ], { quiet: true }) })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', 'lodash', 'package.json'))
    t.ok(_.version === '4.0.0', 'lodash is 4.0.0')
    t.end()
  }, t.end)
})

test('big with dependencies and circular deps (babel-preset-2015)', function (t) {
  prepare()
  install(['babel-preset-es2015@6.3.13'], { quiet: true })
  .then(function () {
    var b = require(join(process.cwd(), 'node_modules', 'babel-preset-es2015'))
    t.ok(typeof b === 'object', 'babel-preset-es2015 is available')
    t.end()
  }, t.end)
})

// NOTE: fsevents can't be installed on Windows
if (!isWindows) {
  test('bundleDependencies (fsevents@1.0.6)', function (t) {
    prepare()
    install(['fsevents@1.0.6'], { quiet: true })
    .then(function () {
      isExecutable(t, join(process.cwd(), 'node_modules', 'fsevents', 'node_modules', '.bin', 'mkdirp'))
      t.end()
    }, t.end)
  })
}

test('compiled modules (ursa@0.9.1)', function (t) {
  if (!process.env.CI || isWindows) {
    t.skip('only ran on CI')
    return t.end()
  }

  prepare()
  install(['ursa@0.9.1'], { quiet: false })
  .then(function () {
    var ursa = require(join(process.cwd(), 'node_modules', 'ursa'))
    t.ok(typeof ursa === 'object', 'ursa() is available')
    t.end()
  }, t.end)
})

test('tarballs (is-array-1.0.1.tgz)', function (t) {
  prepare()
  install(['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], { quiet: true })
  .then(function () {
    var isArray = require(
      join(process.cwd(), 'node_modules', 'is-array'))

    t.ok(isArray, 'isArray() is available')

    stat = fs.statSync(
      join(process.cwd(), 'node_modules', '.store',
        'is-array-1.0.1#a83102a9c117983e6ff4d85311fb322231abe3d6'))
    t.ok(stat.isDirectory(), 'stored in the proper location')
    t.end()
  }, t.end)
})

test('local file', function (t) {
  prepare()
  install([local('local-pkg')], { quiet: true })
  .then(function () {
    var localPkg = require(
      join(process.cwd(), 'node_modules', 'local-pkg'))

    t.ok(localPkg, 'localPkg() is available')

    t.end()
  }, t.end)
})

// Skipping on CI as failing frequently there, due to environment issues
if (!process.env.CI) {
  test('from a github repo', function (t) {
    prepare()
    install(['kevva/is-negative'], { quiet: true })
    .then(function () {
      var localPkg = require(
        join(process.cwd(), 'node_modules', 'is-negative'))

      t.ok(localPkg, 'isNegative() is available')

      t.end()
    }, t.end)
  })
}

test('shrinkwrap compatibility', function (t) {
  prepare({ dependencies: { rimraf: '*' } })

  install(['rimraf@2.5.1'], { quiet: true })
  .then(function () {
    var npm = JSON.stringify(require.resolve('npm/bin/npm-cli.js'))
    require('child_process').exec('node ' + npm + ' shrinkwrap', function (err) {
      if (err) return t.end(err)
      var wrap = JSON.parse(fs.readFileSync('npm-shrinkwrap.json', 'utf-8'))
      t.ok(wrap.dependencies.rimraf.version === '2.5.1',
        'npm shrinkwrap is successful')
      t.end()
    })
  }, t.end)
})

test('run pre/postinstall scripts', function (t) {
  prepare()
  install([local('pre-and-postinstall-scripts-example')], { quiet: true })
  .then(function () {
    var generatedByPreinstall = require(join(process.cwd(), 'node_modules', 'pre-and-postinstall-scripts-example/generated-by-preinstall'))
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    var generatedByPostinstall = require(join(process.cwd(), 'node_modules', 'pre-and-postinstall-scripts-example/generated-by-postinstall'))
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')

    t.end()
  }, t.end)
})

test('run install scripts', function (t) {
  prepare()
  install([local('install-script-example')], { quiet: true })
  .then(function () {
    var generatedByInstall = require(join(process.cwd(), 'node_modules', 'install-script-example/generated-by-install'))
    t.ok(typeof generatedByInstall === 'function', 'generatedByInstall() is available')

    t.end()
  }, t.end)
})

test('save to package.json (rimraf@2.5.1)', function (t) {
  prepare()
  install(['rimraf@2.5.1'], { quiet: true, save: true })
  .then(function () {
    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf() is available')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var dependencies = JSON.parse(pkgJson).dependencies
    t.deepEqual(dependencies, {rimraf: '^2.5.1'}, 'rimraf has been added to dependencies')

    t.end()
  }, t.end)
})

test('saveDev scoped module to package.json (@rstacruz/tap-spec)', function (t) {
  prepare()
  install(['@rstacruz/tap-spec'], { quiet: true, saveDev: true })
  .then(function () {
    var tapSpec = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof tapSpec === 'function', 'tapSpec() is available')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var devDependencies = JSON.parse(pkgJson).devDependencies
    t.deepEqual(devDependencies, { '@rstacruz/tap-spec': '^4.1.1' }, 'tap-spec has been added to devDependencies')

    t.end()
  }, t.end)
})

test('multiple save to package.json with `exact` versions (@rstacruz/tap-spec & rimraf@2.5.1) (in sorted order)', function (t) {
  prepare()
  install(['rimraf@2.5.1', '@rstacruz/tap-spec@latest'], { quiet: true, save: true, saveExact: true })
  .then(function () {
    var tapSpec = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof tapSpec === 'function', 'tapSpec() is available')

    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf() is available')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var dependencies = JSON.parse(pkgJson).dependencies
    var expectedDeps = {
      '@rstacruz/tap-spec': '4.1.1',
      rimraf: '2.5.1'
    }
    t.deepEqual(dependencies, expectedDeps, 'tap-spec and rimraf have been added to dependencies')
    t.deepEqual(Object.keys(dependencies), Object.keys(expectedDeps), 'tap-spec and rimraf have been added to dependencies in sorted order')

    t.end()
  }, t.end)
})

test('flattening symlinks (minimatch@3.0.0)', function (t) {
  prepare()
  install(['minimatch@3.0.0'], { quiet: true })
  .then(function () {
    stat = fs.lstatSync(join(process.cwd(), 'node_modules', '.store', 'node_modules', 'balanced-match'))
    t.ok(stat.isSymbolicLink(), 'balanced-match is linked into store node_modules')

    _ = exists(join(process.cwd(), 'node_modules', 'balanced-match'))
    t.ok(!_, 'balanced-match is not linked into main node_modules')
    t.end()
  }, t.end)
})

test('flattening symlinks (minimatch + balanced-match)', function (t) {
  prepare()
  install(['minimatch@3.0.0'], { quiet: true })
  .then(function () {
    return install(['balanced-match@^0.3.0'], { quiet: true })
  })
  .then(function () {
    _ = exists(join(process.cwd(), 'node_modules', '.store', 'node_modules', 'balanced-match'))
    t.ok(!_, 'balanced-match is removed from store node_modules')

    _ = exists(join(process.cwd(), 'node_modules', 'balanced-match'))
    t.ok(_, 'balanced-match now in main node_modules')
    t.end()
  }, t.end)
})

test('production install (with --production flag)', function (t) {
  prepare(basicPackageJson)

  return install([], { quiet: true, production: true })
    .then(function () {
      var rimrafDir = fs.statSync(join(process.cwd(), 'node_modules', 'rimraf'))

      var tapStatErrCode
      try {
        fs.statSync(join(process.cwd(), 'node_modules', '@rstacruz'))
      } catch (err) { tapStatErrCode = err.code }

      t.ok(rimrafDir.isSymbolicLink, 'rimraf exists')
      t.is(tapStatErrCode, 'ENOENT', 'tap-spec does not exist')

      t.end()
    }, t.end)
})

test('production install (with production NODE_ENV)', function (t) {
  var originalNodeEnv = process.env.NODE_ENV
  process.env.NODE_ENV = 'production'
  prepare(basicPackageJson)

  return install([], { quiet: true })
    .then(function () {
      // reset NODE_ENV
      process.env.NODE_ENV = originalNodeEnv

      var rimrafDir = fs.statSync(join(process.cwd(), 'node_modules', 'rimraf'))

      var tapStatErrCode
      try {
        fs.statSync(join(process.cwd(), 'node_modules', '@rstacruz'))
      } catch (err) { tapStatErrCode = err.code }

      t.ok(rimrafDir.isSymbolicLink, 'rimraf exists')
      t.is(tapStatErrCode, 'ENOENT', 'tap-spec does not exist')

      t.end()
    }, t.end)
})

test('uninstall package with no dependencies', function (t) {
  prepare()
  install(['is-negative@2.1.0'], { quiet: true, save: true })
  .then(_ => uninstall(['is-negative'], { save: true }))
  .then(function () {
    stat = exists(join(process.cwd(), 'node_modules', '.store', 'is-negative@2.1.0'))
    t.ok(!stat, 'is-negative is removed from store')

    stat = existsSymlink(join(process.cwd(), 'node_modules', 'is-negative'))
    t.ok(!stat, 'is-negative is removed from node_modules')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var dependencies = JSON.parse(pkgJson).dependencies
    var expectedDeps = {}
    t.deepEqual(dependencies, expectedDeps, 'is-negative has been removed from dependencies')

    t.end()
  }, t.end)
})

test('uninstall package with dependencies and do not touch other deps', function (t) {
  prepare()
  install(['is-negative@2.1.0', 'camelcase-keys@3.0.0'], { quiet: true, save: true })
  .then(_ => uninstall(['camelcase-keys'], { save: true }))
  .then(function () {
    stat = exists(join(process.cwd(), 'node_modules', '.store', 'camelcase-keys@2.1.0'))
    t.ok(!stat, 'camelcase-keys is removed from store')

    stat = existsSymlink(join(process.cwd(), 'node_modules', 'camelcase-keys'))
    t.ok(!stat, 'camelcase-keys is removed from node_modules')

    stat = exists(join(process.cwd(), 'node_modules', '.store', 'camelcase@3.0.0'))
    t.ok(!stat, 'camelcase is removed from store')

    stat = existsSymlink(join(process.cwd(), 'node_modules', 'camelcase'))
    t.ok(!stat, 'camelcase is removed from node_modules')

    stat = exists(join(process.cwd(), 'node_modules', '.store', 'map-obj@1.0.1'))
    t.ok(!stat, 'map-obj is removed from store')

    stat = existsSymlink(join(process.cwd(), 'node_modules', 'map-obj'))
    t.ok(!stat, 'map-obj is removed from node_modules')

    stat = exists(join(process.cwd(), 'node_modules', '.store', 'is-negative@2.1.0'))
    t.ok(stat, 'is-negative is not removed from store')

    stat = existsSymlink(join(process.cwd(), 'node_modules', 'is-negative'))
    t.ok(stat, 'is-negative is not removed from node_modules')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var dependencies = JSON.parse(pkgJson).dependencies
    var expectedDeps = {
      'is-negative': '^2.1.0'
    }
    t.deepEqual(dependencies, expectedDeps, 'camelcase-keys has been removed from dependencies')

    t.end()
  }, t.end)
})

test('uninstall package with its bin files', function (t) {
  prepare()
  install(['sh-hello-world@1.0.0'], { quiet: true, save: true })
  .then(_ => uninstall(['sh-hello-world'], { save: true }))
  .then(function () {
    // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
    stat = existsSymlink(join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
    t.ok(!stat, 'sh-hello-world is removed from .bin')

    stat = exists(join(process.cwd(), 'node_modules', '.bin', 'sh-hello-world'))
    t.ok(!stat, 'sh-hello-world is removed from .bin')

    t.end()
  }, t.end)
})

test('keep dependencies used by others', function (t) {
  prepare()
  install(['hastscript@3.0.0', 'camelcase-keys@3.0.0'], { quiet: true, save: true })
  .then(_ => uninstall(['camelcase-keys'], { save: true }))
  .then(function () {
    stat = exists(join(process.cwd(), 'node_modules', '.store', 'camelcase-keys@2.1.0'))
    t.ok(!stat, 'camelcase-keys is removed from store')

    stat = existsSymlink(join(process.cwd(), 'node_modules', 'camelcase-keys'))
    t.ok(!stat, 'camelcase-keys is removed from node_modules')

    stat = exists(join(process.cwd(), 'node_modules', '.store', 'camelcase@3.0.0'))
    t.ok(stat, 'camelcase is not removed from store')

    stat = exists(join(process.cwd(), 'node_modules', '.store', 'map-obj@1.0.1'))
    t.ok(!stat, 'map-obj is removed from store')

    stat = existsSymlink(join(process.cwd(), 'node_modules', 'map-obj'))
    t.ok(!stat, 'map-obj is removed from node_modules')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var dependencies = JSON.parse(pkgJson).dependencies
    var expectedDeps = {
      'hastscript': '^3.0.0'
    }
    t.deepEqual(dependencies, expectedDeps, 'camelcase-keys has been removed from dependencies')

    t.end()
  }, t.end)
})

const wait = ms => new Promise(resolve => setTimeout(resolve, ms))

test('fail when trying to install into the same store simultaneously', function (t) {
  prepare()
  Promise.all([
    install([local('pkg-that-installs-slowly')], { quiet: true }),
    wait(500) // to be sure that lock was created
      .then(_ => install(['rimraf@2.5.1'], { quiet: true }))
      .then(_ => t.fail('the store should have been locked'))
      .catch(err => t.ok(err, 'store is locked'))
  ])
  .then(_ => t.end(), t.end)
})

test('fail when trying to install and uninstall from the same store simultaneously', function (t) {
  prepare()
  Promise.all([
    install([local('pkg-that-installs-slowly')], { quiet: true }),
    wait(500) // to be sure that lock was created
      .then(_ => uninstall(['rimraf@2.5.1'], { quiet: true }))
      .then(_ => t.fail('the store should have been locked'))
      .catch(err => t.ok(err, 'store is locked'))
  ])
  .then(_ => t.end(), t.end)
})

if (preserveSymlinks) {
  test('packages should find the plugins they use when symlinks are preserved', function (t) {
    prepare()
    install([local('pkg-that-uses-plugins'), local('plugin-example')], { quiet: true, save: true })
      .then(_ => {
        var result = spawnSync('pkg-that-uses-plugins', [], {
          env: extendPathWithLocalBin()
        })
        t.equal(result.stdout.toString(), 'plugin-example\n', 'package executable have found its plugin')
        t.equal(result.status, 0, 'executable exited with success')
        t.end()
      }, t.end)
  })
}

test('run js bin file', function (t) {
  prepare()
  install([local('hello-world-js-bin')], { quiet: true, save: true })
    .then(_ => {
      var result = spawnSync('hello-world-js-bin', [], {
        env: extendPathWithLocalBin()
      })
      t.equal(result.stdout.toString(), 'Hello world!\n', 'package executable printed its message')
      t.equal(result.status, 0, 'executable exited with success')
      t.end()
    })
    .catch(t.end)
})

const pnpmBin = join(__dirname, '../bin/pnpm.js')

test('installation via the CLI', t => {
  prepare()
  const result = spawnSync('node', [pnpmBin, 'install', 'rimraf@2.5.1'])

  t.equal(result.status, 0, 'install successful')

  const rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
  t.ok(typeof rimraf === 'function', 'rimraf() is available')

  isExecutable(t, join(process.cwd(), 'node_modules', '.bin', 'rimraf'))

  t.end()
})

test('pass through to npm CLI for commands that are not supported by npm', t => {
  const result = spawnSync('node', [pnpmBin, 'config', 'get', 'user-agent'])

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

  const result = spawnSync('node', [pnpmBin, 'install', 'is-negative'])

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

  const result = spawnSync('node', [pnpmBin, 'install', 'is-negative'])

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'prepublish script was executed')

  t.end()
})

test('global installation', t => {
  const globalPath = join(process.cwd(), '.tmp', 'global')
  install(['is-positive'], {quiet: true, globalPath, global: true})
    .then(_ => {
      const isPositive = require(join(globalPath, 'node_modules', 'is-positive'))
      t.ok(typeof isPositive === 'function', 'isPositive() is available')

      t.end()
    })
    .catch(t.end)
})

function extendPathWithLocalBin () {
  return {
    PATH: [
      join(process.cwd(), 'node_modules', '.bin'),
      process.env.PATH
    ].join(delimiter)
  }
}

function local (pkgName) {
  var pkgPath = join(__dirname, 'packages', pkgName)
  return 'file:' + pkgPath
}

function exists (path) {
  try {
    return fs.statSync(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
}

function existsSymlink (path) {
  try {
    return fs.lstatSync(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
}
