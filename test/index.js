var test = require('tape')
var join = require('path').join
var prepare = require('./support/prepare')
var install = require('../bin/pnpm-install')
require('./support/sepia')

test('small with dependencies (rimraf)', function (t) {
  prepare()
  install({ input: ['rimraf@2.5.1'] })
  .then(function () {
    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf is available')
    t.end()
  }, t.end)
})

test('no dependencies (lodash)', function (t) {
  prepare()
  install({ input: ['lodash@4.0.0'] })
  .then(function () {
    var _ = require(join(process.cwd(), 'node_modules', 'lodash'))
    t.ok(typeof _ === 'function', '_ is available')
    t.ok(typeof _.clone === 'function', '_.clone is available')
    t.end()
  }, t.end)
})

test('idempotency (rimraf)', function (t) {
  prepare()
  install({ input: ['rimraf@2.5.1'] })
  .then(function () { return install({ input: [ 'rimraf@2.5.1' ] }) })
  .then(function () {
    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf is available')
    t.end()
  }, t.end)
})

test('big with dependencies (babel-preset-2015)', function (t) {
  prepare()
  install({ input: ['babel-preset-es2015@6.3.13'] })
  .then(function () {
    var b = require(join(process.cwd(), 'node_modules', 'babel-preset-es2015'))
    t.ok(typeof b === 'object', 'babel-preset-es2015 is available')
    t.end()
  }, t.end)
})
