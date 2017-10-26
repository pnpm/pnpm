import path = require('path')
import fs = require('mz/fs')
import tape = require('tape')
import loadJsonFile = require('load-json-file')
import promisifyTape from 'tape-promise'
import {
  prepare,
  testDefaults,
} from '../utils'
import {install} from '../../src'

const basicPackageJson = loadJsonFile.sync(path.join(__dirname, '../utils/simple-package.json'))
const test = promisifyTape(tape)

test('production install (with --production flag)', async function (t) {
  const project = prepare(t, basicPackageJson)

  await install(testDefaults({ production: true }))

  const rimrafDir = fs.statSync(path.resolve('node_modules', 'rimraf'))

  let tapStatErrCode: number = 0
  try {
    fs.statSync(path.resolve('node_modules', '@rstacruz'))
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

  const rimrafDir = fs.statSync(path.resolve('node_modules', 'rimraf'))

  let tapStatErrCode: number = 0
  try {
    fs.statSync(path.resolve('node_modules', '@rstacruz'))
  } catch (err) { tapStatErrCode = err.code }

  t.ok(rimrafDir.isSymbolicLink, 'rimraf exists')
  t.is(tapStatErrCode, 'ENOENT', 'tap-spec does not exist')
})
