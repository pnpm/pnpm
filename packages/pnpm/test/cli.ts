import prepare from '@pnpm/prepare'
import execa = require('execa')
import makeDir = require('make-dir')
import fs = require('mz/fs')
import rimraf = require('rimraf-then')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  execPnpm,
  execPnpmSync,
} from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('returns help when not available command is used', t => {
  const result = execPnpmSync('foobarqar')

  t.equal(result.status, 0)
  t.ok(result.stdout.toString().includes('Usage: pnpm [command] [flags]'))

  t.end()
})

test('some commands pass through to npm', t => {
  const result = execPnpmSync('dist-tag', 'ls', 'is-positive')

  t.equal(result.status, 0)
  t.ok(!result.stdout.toString().includes('Usage: pnpm [command] [flags]'))

  t.end()
})

test('installs in the folder where the package.json file is', async function (t) {
  const project = prepare(t)

  await makeDir('subdir')
  process.chdir('subdir')

  await execPnpm('install', 'rimraf@2.5.1')

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')
})

test('pnpm import does not move modules created by npm', async (t: tape.Test) => {
  const project = prepare(t)

  await execa('npm', ['install', 'is-positive@1.0.0', '--save'])
  await execa('npm', ['shrinkwrap'])

  const packageJsonInodeBefore = (await fs.stat('node_modules/is-positive/package.json')).ino

  await execPnpm('import')

  const packageJsonInodeAfter = (await fs.stat('node_modules/is-positive/package.json')).ino

  t.equal(packageJsonInodeBefore, packageJsonInodeAfter)
})

test('installation via the CLI', async function (t) {
  const project = prepare(t)
  const result = execPnpmSync('install', 'rimraf@2.5.1')

  t.equal(result.status, 0, 'install successful')

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')

  await project.isExecutable('.bin/rimraf')
})

test('pass through to npm CLI for commands that are not supported by npm', t => {
  const result = execPnpmSync('config', 'get', 'user-agent')

  t.equal(result.status, 0, 'command was successfull')
  t.ok(result.stdout.toString().includes('npm/'), 'command returned correct result')

  t.end()
})

test('pass through to npm with all the args', async t => {
  const project = prepare(t)
  await rimraf('package.json')

  const result = execPnpmSync('init', '-y')

  t.equal(result.status, 0, 'command was successfull')
})
