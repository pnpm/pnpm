var test = require('tape')
var join = require('path').join
var fs = require('fs')
var prepare = require('./support/prepare')
var install = require('../lib/cmd/install')
require('./support/sepia')

var stat, _

test('eslint', require('tape-eslint')())

test('small with dependencies (rimraf)', function (t) {
  prepare()
  install({ input: ['rimraf@2.5.1'], flags: { quiet: true } })
  .then(function () {
    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf() is available')

    stat = fs.lstatSync(join(process.cwd(), 'node_modules', '.bin', 'rimraf'))
    t.ok(stat.isSymbolicLink(), '.bin/rimraf symlink is available')

    stat = fs.statSync(join(process.cwd(), 'node_modules', 'rimraf', 'bin.js'))
    t.equal(stat.mode, 0o100755, 'rimraf is executable')
    t.ok(stat.isFile(), '.bin/rimraf refers to a file')

    t.end()
  }, t.end)
})

test('no dependencies (lodash)', function (t) {
  prepare()
  install({ input: ['lodash@4.0.0'], flags: { quiet: true } })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', 'lodash'))
    t.ok(typeof _ === 'function', '_ is available')
    t.ok(typeof _.clone === 'function', '_.clone is available')
    t.end()
  }, t.end)
})

test('scoped modules without version spec (@rstacruz/tap-spec)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec'], flags: { quiet: true } })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  }, t.end)
})

test('scoped modules with versions (@rstacruz/tap-spec@4.1.1)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec@4.1.1'], flags: { quiet: true } })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  }, t.end)
})

test('scoped modules (@rstacruz/tap-spec@*)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec@*'], flags: { quiet: true } })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  }, t.end)
})

test('multiple scoped modules (@rstacruz/...)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], flags: { quiet: true } })
  .then(function () {
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    _ = require(join(process.cwd(), 'node_modules', '@rstacruz/travis-encrypt'))
    t.ok(typeof _ === 'function', 'travis-encrypt is available')
    t.end()
  }, t.end)
})

test('idempotency (rimraf)', function (t) {
  prepare()
  install({ input: ['rimraf@2.5.1'], flags: { quiet: true } })
  .then(function () { return install({ input: [ 'rimraf@2.5.1' ], flags: { quiet: true } }) })
  .then(function () {
    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf is available')
    t.end()
  }, t.end)
})

test('big with dependencies and circular deps (babel-preset-2015)', function (t) {
  prepare()
  install({ input: ['babel-preset-es2015@6.3.13'], flags: { quiet: true } })
  .then(function () {
    var b = require(join(process.cwd(), 'node_modules', 'babel-preset-es2015'))
    t.ok(typeof b === 'object', 'babel-preset-es2015 is available')
    t.end()
  }, t.end)
})

test('bundleDependencies (fsevents@1.0.6)', function (t) {
  prepare()
  install({ input: ['fsevents@1.0.6'], flags: { quiet: true } })
  .then(function () {
    stat = fs.lstatSync(
      join(process.cwd(), 'node_modules', 'fsevents', 'node_modules', '.bin', 'mkdirp'))
    t.ok(stat.isSymbolicLink(), '.bin/mkdirp is available')

    stat = fs.statSync(
      join(process.cwd(), 'node_modules', 'fsevents', 'node_modules', '.bin', 'mkdirp'))
    t.ok(stat.isFile(), '.bin/mkdirp refers to a file')
    t.end()
  }, t.end)
})

test('compiled modules (ursa@0.9.1)', function (t) {
  if (!process.env.CI) {
    t.skip('only ran on CI')
    return t.end()
  }

  prepare()
  install({ input: ['ursa@0.9.1'], flags: { quiet: false } })
  .then(function () {
    var ursa = require(join(process.cwd(), 'node_modules', 'ursa'))
    t.ok(typeof ursa === 'object', 'ursa() is available')
    t.end()
  }, t.end)
})

test('tarballs (is-array-1.0.1.tgz)', function (t) {
  prepare()
  install({ input: ['http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz'], flags: { quiet: true } })
  .then(function () {
    var isArray = require(
      join(process.cwd(), 'node_modules', 'is-array'))

    t.ok(isArray, 'isArray() is available')

    stat = fs.statSync(
      join(process.cwd(), 'node_modules', '.store',
        'is-array-1.0.1@a83102a9c117983e6ff4d85311fb322231abe3d6'))
    t.ok(stat.isDirectory(), 'stored in the proper location')
    t.end()
  }, t.end)
})

test('shrinkwrap compatibility', function (t) {
  prepare()
  fs.writeFileSync('package.json',
    JSON.stringify({ dependencies: { rimraf: '*' } }),
    'utf-8')

  install({ input: ['rimraf@2.5.1'], flags: { quiet: true } })
  .then(function () {
    var npm = JSON.stringify(require.resolve('npm/bin/npm-cli.js'))
    require('child_process').execSync('node ' + npm + ' shrinkwrap')
    var wrap = JSON.parse(fs.readFileSync('npm-shrinkwrap.json', 'utf-8'))
    t.ok(wrap.dependencies.rimraf.version === '2.5.1',
      'npm shrinkwrap is successful')
    t.end()
  }, t.end)
})

test('save to package.json (rimraf@2.5.1)', function (t) {
  prepare()
  install({ input: ['rimraf@2.5.1'], flags: { quiet: true, save: true } })
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
  install({ input: ['@rstacruz/tap-spec'], flags: { quiet: true, saveDev: true } })
  .then(function () {
    var tapSpec = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof tapSpec === 'function', 'tapSpec() is available')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var devDependencies = JSON.parse(pkgJson).devDependencies
    t.deepEqual(devDependencies, { '@rstacruz/tap-spec': '^4.1.1' }, 'tap-spec has been added to devDependencies')

    t.end()
  }, t.end)
})

test('multiple save to package.json with `exact` versions (@rstacruz/tap-spec & rimraf@2.5.1)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec@latest', 'rimraf@2.5.1'], flags: { quiet: true, save: true, saveExact: true } })
  .then(function () {
    var tapSpec = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof tapSpec === 'function', 'tapSpec() is available')

    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf() is available')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var dependencies = JSON.parse(pkgJson).dependencies
    var expectedDeps = {
      rimraf: '2.5.1',
      '@rstacruz/tap-spec': '4.1.1'
    }
    t.deepEqual(dependencies, expectedDeps, 'tap-spec and rimraf have been added to dependencies')

    t.end()
  }, t.end)
})

test('flattening symlinks (minimatch@3.0.0)', function (t) {
  prepare()
  install({ input: ['minimatch@3.0.0'], flags: { quiet: true } })
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
  install({ input: ['minimatch@3.0.0'], flags: { quiet: true } })
  .then(_ => install({ input: ['balanced-match@^0.3.0'], flags: { quiet: true } }))
  .then(function () {
    _ = exists(join(process.cwd(), 'node_modules', '.store', 'node_modules', 'balanced-match'))
    t.ok(!_, 'balanced-match is removed from store node_modules')

    _ = exists(join(process.cwd(), 'node_modules', 'balanced-match'))
    t.ok(_, 'balanced-match now in main node_modules')
    t.end()
  }, t.end)
})

function exists (path) {
  try {
    return fs.statSync(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
}
