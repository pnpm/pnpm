import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import execa = require('execa')
import exists = require('path-exists')
import mkdirp = require('mkdirp-promise')
import {
  prepare,
  addDistTag,
  execPnpm,
  execPnpmSync,
} from './utils'
import rimraf = require('rimraf-then')

test('return error status code when underlying command fails', t => {
  const result = execPnpmSync('invalid-command')

  t.equal(result.status, 1, 'error status code returned')

  t.end()
})

test('installs in the folder where the package.json file is', async function (t) {
  const project = prepare(t)

  await mkdirp('subdir')
  process.chdir('subdir')

  await execPnpm('install', 'rimraf@2.5.1')

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')
})

test('rewrites node_modules created by npm', async function (t) {
  const project = prepare(t)

  await execa('npm', ['install', 'rimraf@2.5.1', '@types/node', '--save'])

  await execPnpm('install')

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')
})

test('update', async function (t) {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await execPnpm('install', 'pkg-with-1-dep', '-S')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm('update', '--depth', '1')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
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
  t.ok(result.stdout.toString().indexOf('npm/') !== -1, 'command returned correct result')

  t.end()
})

test('pass through to npm with all the args', async t => {
  const project = prepare(t)
  await rimraf('package.json')

  const result = execPnpmSync('init', '-y')

  t.equal(result.status, 0, 'command was successfull')
})
