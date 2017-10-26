import path = require('path')
import fs = require('mz/fs')
import tape = require('tape')
import loadJsonFile = require('load-json-file')
import promisifyTape from 'tape-promise'
import {
  prepare,
  testDefaults,
  execPnpm,
} from '../utils'

const basicPackageJson = loadJsonFile.sync(path.join(__dirname, '../utils/simple-package.json'))
const test = promisifyTape(tape)

test('production install (with --production flag)', async (t: tape.Test) => {
  const project = prepare(t, basicPackageJson)

  await execPnpm('install', '--production')

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

test('production install (with production NODE_ENV)', async (t: tape.Test) => {
  const originalNodeEnv = process.env.NODE_ENV
  process.env.NODE_ENV = 'production'
  const project = prepare(t, basicPackageJson)

  await execPnpm('install')

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

test('install dev dependencies only', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': "^1.0.0",
    },
    devDependencies: {
      'is-negative': "^1.0.0",
    },
  })

  // NODE_ENV should be ignored if --only is used
  const originalNodeEnv = process.env.NODE_ENV
  process.env.NODE_ENV = 'production'

  await execPnpm('install', '--only', 'dev')

  // reset NODE_ENV
  process.env.NODE_ENV = originalNodeEnv

  const isNegative = project.requireModule('is-negative')
  t.equal(typeof isNegative, 'function', 'dev dependency is available')

  await project.hasNot('is-positive')
})
