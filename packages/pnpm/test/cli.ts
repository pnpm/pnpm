import prepare from '@pnpm/prepare'
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import makeDir = require('make-dir')
import fs = require('mz/fs')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  execPnpm,
  execPnpmSync,
  execPnpxSync,
} from './utils'

const test = promisifyTape(tape)

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

  const packageManifestInodeBefore = (await fs.stat('node_modules/is-positive/package.json')).ino

  await execPnpm('import')

  const packageManifestInodeAfter = (await fs.stat('node_modules/is-positive/package.json')).ino

  t.equal(packageManifestInodeBefore, packageManifestInodeAfter)
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

test('pnpm fails when an unsupported command is used', async (t) => {
  const project = prepare(t)

  const { status, stderr } = execPnpmSync('unsupported-command')

  t.equal(status, 1, 'command failed')
  t.ok(stderr.toString().includes("Unknown command 'unsupported-command'"))
})

test('pnpm fails when no command is specified', async (t) => {
  const project = prepare(t)

  const { status, stdout } = execPnpmSync()

  t.equal(status, 1, 'command failed')
  t.ok(stdout.toString().includes('Usage:'))
})

test('command fails when an unsupported flag is used', async (t) => {
  const project = prepare(t)

  const { status, stderr } = execPnpmSync('update', '--save-dev')

  t.equal(status, 1, 'command failed')
  t.ok(stderr.toString().includes("Unknown option 'save-dev'"))
})

test('adding new dep does not fail if node_modules was created with --no-hoist and --independent-leaves', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('add', 'is-positive', '--no-hoist', '--independent-leaves')

  t.equal(execPnpmSync('add', 'is-negative', '--hoist').status, 1)
  t.equal(execPnpmSync('add', 'is-negative', '--no-independent-leaves').status, 1)
  t.equal(execPnpmSync('add', 'is-negative').status, 0)

  await project.has('is-negative')
})

test('adding new dep does not fail if node_modules was created with --hoist-pattern=eslint-* and --shamefully-hoist', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('add', 'is-positive', '--hoist-pattern=eslint-*', '--shamefully-hoist')

  t.equal(execPnpmSync('add', 'is-negative', '--no-hoist').status, 1)
  t.equal(execPnpmSync('add', 'is-negative', '--no-shamefully-hoist').status, 1)
  t.equal(execPnpmSync('add', 'is-negative').status, 0)

  await project.has('is-negative')
})

test('pnpx works', t => {
  const result = execPnpxSync('hello-world-js-bin')

  t.equal(result.status, 0)
  t.ok(result.stdout.toString().includes('Hello world!'))

  t.end()
})
