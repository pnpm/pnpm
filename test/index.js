var test = require('tape')
var join = require('path').join
var fs = require('fs')
var prepare = require('./support/prepare')
var install = require('../bin/pnpm-install')
require('./support/sepia')
var stat

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
    var _ = require(join(process.cwd(), 'node_modules', 'lodash'))
    t.ok(typeof _ === 'function', '_ is available')
    t.ok(typeof _.clone === 'function', '_.clone is available')
    t.end()
  }, t.end)
})

test('scoped modules without version spec (@rstacruz/tap-spec)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec'], flags: { quiet: true } })
  .then(function () {
    var _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  }, t.end)
})

test('scoped modules with versions (@rstacruz/tap-spec@4.1.1)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec@4.1.1'], flags: { quiet: true } })
  .then(function () {
    var _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  }, t.end)
})

test('scoped modules (@rstacruz/tap-spec@*)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec@*'], flags: { quiet: true } })
  .then(function () {
    var _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')
    t.end()
  }, t.end)
})

test('multiple scoped modules (@rstacruz/...)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec@*', '@rstacruz/travis-encrypt@*'], flags: { quiet: true } })
  .then(function () {
    var _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
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

test('save to package.json (rimraf@2.5.1)', function (t) {
  prepare()
  install({ input: ['rimraf@2.5.1'], flags: { quiet: true, save: true } })
  .then(function () {
    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf() is available')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var dependencies = JSON.parse(pkgJson).dependencies
    t.deepEqual(dependencies, {rimraf: '2.5.1'}, 'rimraf has been added to dependencies')

    t.end()
  }, t.end)
})

test('saveDev scoped module to package.json (@rstacruz/tap-spec)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec'], flags: { quiet: true, saveDev: true } })
  .then(function () {
    var _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var devDependencies = JSON.parse(pkgJson).devDependencies
    t.deepEqual(devDependencies, { '@rstacruz/tap-spec': '^4.1.1' }, 'tap-spec has been added to devDependencies')

    t.end()
  }, t.end)
})

test.only('multiple mixed save to package.json (@rstacruz/tap-spec & rimraf@2.5.1)', function (t) {
  prepare()
  install({ input: ['@rstacruz/tap-spec', 'rimraf@2.5.1'], flags: { quiet: true, save: true } })
  .then(function () {
    var _ = require(join(process.cwd(), 'node_modules', '@rstacruz/tap-spec'))
    t.ok(typeof _ === 'function', 'tap-spec is available')

    var pkgJson = fs.readFileSync(join(process.cwd(), 'package.json'), 'utf8')
    var dependencies = JSON.parse(pkgJson).dependencies
    var expectedDeps = {
      rimraf: '2.5.1',
      '@rstacruz/tap-spec': '^4.1.1'
    }
    t.deepEqual(dependencies, expectedDeps, 'tap-spec has been added to devDependencies')

    t.end()
  }, t.end)
})
