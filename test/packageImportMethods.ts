import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import exists = require('path-exists')
import {
  prepare,
  testDefaults,
  addDistTag,
} from './utils'
import {
  installPkgs,
  install,
  RootLog,
} from 'supi'
import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import {stripIndent} from 'common-tags'
import fs = require('mz/fs')

const test = promisifyTape(tape)

test('packageImportMethod can be set to copy', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative'], await testDefaults({}, {}, {}, {packageImportMethod: 'copy'}))

  const m = project.requireModule('is-negative')
  t.ok(m, 'is-negative is available with packageImportMethod = copy')
})
