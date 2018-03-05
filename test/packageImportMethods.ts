import {stripIndent} from 'common-tags'
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import path = require('path')
import exists = require('path-exists')
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import {
  install,
  installPkgs,
  RootLog,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writePkg = require('write-pkg')
import writeYamlFile = require('write-yaml-file')
import {
  addDistTag,
  prepare,
  testDefaults,
} from './utils'

const test = promisifyTape(tape)

test('packageImportMethod can be set to copy', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative'], await testDefaults({}, {}, {}, {packageImportMethod: 'copy'}))

  const m = project.requireModule('is-negative')
  t.ok(m, 'is-negative is available with packageImportMethod = copy')
})
