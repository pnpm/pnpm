import 'sepia'
import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import fs = require('mz/fs')
import caw = require('caw')
import semver = require('semver')
import crossSpawn = require('cross-spawn')
const spawnSync = crossSpawn.sync
import isCI = require('is-ci')
import rimraf = require('rimraf-then')
import readPkg = require('read-pkg')
import {
  prepare,
  addDistTag,
  testDefaults,
  execPnpmSync,
} from '../utils'
import loadJsonFile = require('load-json-file')
const basicPackageJson = loadJsonFile.sync(path.join(__dirname, '../utils/simple-package.json'))
import exists = require('path-exists')
import isWindows = require('is-windows')

const IS_WINDOWS = isWindows()

if (!caw() && !IS_WINDOWS) {
  process.env.VCR_MODE = 'cache'
}

test('bin files are found by lifecycle scripts', t => {
  const project = prepare(t, {
    scripts: {
      postinstall: 'hello-world-js-bin'
    },
    dependencies: {
      'hello-world-js-bin': '*'
    }
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().indexOf('Hello world!') !== -1, 'postinstall script was executed')

  t.end()
})

test('create a pnpm-debug.log file when the command fails', async function (t) {
  const project = prepare(t)

  const result = execPnpmSync('install', '@zkochan/i-do-not-exist')

  t.equal(result.status, 1, 'install failed')

  t.ok(await exists('pnpm-debug.log'), 'log file created')

  t.end()
})
