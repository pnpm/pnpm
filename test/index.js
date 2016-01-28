var test = require('tape')
var join = require('path').join
var prepare = require('./support/prepare')
require('./support/sepia')

test('small with dependencies (rimraf)', function (t) {
  prepare()
  require('../bin/unpm-install')({
    input: ['rimraf@2.5.1']
  })
  .then(function (res) {
    var rimraf = require(join(process.cwd(), 'node_modules', 'rimraf'))
    t.ok(typeof rimraf === 'function', 'rimraf is available')
    t.end()
  }, t.end)
})

test('no dependencies (lodash)', function (t) {
  prepare()
  require('../bin/unpm-install')({
    input: ['lodash@4.0.0']
  })
  .then(function (res) {
    var _ = require(join(process.cwd(), 'node_modules', 'lodash'))
    t.ok(typeof _ === 'function', '_ is available')
    t.ok(typeof _.clone === 'function', '_.clone is available')
    t.end()
  }, t.end)
})
