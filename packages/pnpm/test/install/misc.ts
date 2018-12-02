import prepare from '@pnpm/prepare'
import caw = require('caw')
import isWindows = require('is-windows')
import path = require('path')
import exists = require('path-exists')
import { Shrinkwrap } from 'pnpm-shrinkwrap'
import readYamlFile from 'read-yaml-file'
import 'sepia'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  execPnpm,
  execPnpmSync,
} from '../utils'

const IS_WINDOWS = isWindows()
const test = promisifyTape(tape)

if (!caw() && !IS_WINDOWS) {
  process.env.VCR_MODE = 'cache'
}

test('bin files are found by lifecycle scripts', t => {
  const project = prepare(t, {
    dependencies: {
      'hello-world-js-bin': '*'
    },
    scripts: {
      postinstall: 'hello-world-js-bin'
    },
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

test('install from any location via the --prefix flag', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      rimraf: '2.6.2',
    },
  })

  process.chdir('..')

  await execPnpm('install', '--prefix', 'project')

  await project.has('rimraf')
  await project.isExecutable('.bin/rimraf')
})

test('install with external shrinkwrap directory', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive', '--shrinkwrap-directory', path.resolve('..'))

  await project.has('is-positive')

  const shr = await readYamlFile<Shrinkwrap>(path.resolve('..', 'shrinkwrap.yaml'))

  t.deepEqual(Object.keys(shr.importers), ['project'], 'shrinkwrap created in correct location')
})
