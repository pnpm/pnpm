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
import {
  prepare,
  addDistTag,
  testDefaults,
  execPnpm,
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

test('install --shrinkwrap-only', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'rimraf@2.5.1', '--shrinkwrap-only')

  await project.hasNot('rimraf')

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/rimraf/2.5.1'])
})

test('install --no-shrinkwrap', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive', '--no-shrinkwrap')

  await project.has('is-positive')

  t.notOk(await project.loadShrinkwrap(), 'shrinkwrap.yaml not created')
})

test('install --no-package-lock', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive', '--no-package-lock')

  await project.has('is-positive')

  t.notOk(await project.loadShrinkwrap(), 'shrinkwrap.yaml not created')
})

test('install from any location via the --prefix flag', async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  process.chdir('..')

  await execPnpm('install', '--prefix', 'project')

  await project.has('is-positive')
})
