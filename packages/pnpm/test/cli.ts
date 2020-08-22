import prepare from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import {
  execPnpm,
  execPnpmSync,
  execPnpxSync,
} from './utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import fs = require('mz/fs')
import tape = require('tape')

const test = promisifyTape(tape)
const fixtures = path.join(__dirname, '../../../fixtures')
const hasOutdatedDepsFixture = path.join(fixtures, 'has-outdated-deps')

test('some commands pass through to npm', t => {
  const result = execPnpmSync(['dist-tag', 'ls', 'is-positive'])

  t.equal(result.status, 0)
  t.ok(!result.stdout.toString().includes('Usage: pnpm [command] [flags]'))

  t.end()
})

test('installs in the folder where the package.json file is', async function (t) {
  const project = prepare(t)

  await fs.mkdir('subdir')
  process.chdir('subdir')

  await execPnpm(['install', 'rimraf@2.5.1'])

  const m = project.requireModule('rimraf')
  t.ok(typeof m === 'function', 'rimraf() is available')
  await project.isExecutable('.bin/rimraf')
})

test('pnpm import does not move modules created by npm', async (t: tape.Test) => {
  prepare(t)

  await execa('npm', ['install', 'is-positive@1.0.0', '--save'])
  await execa('npm', ['shrinkwrap'])

  const packageManifestInodeBefore = (await fs.stat('node_modules/is-positive/package.json')).ino

  await execPnpm(['import'])

  const packageManifestInodeAfter = (await fs.stat('node_modules/is-positive/package.json')).ino

  t.equal(packageManifestInodeBefore, packageManifestInodeAfter)
})

test('pass through to npm CLI for commands that are not supported by npm', t => {
  const result = execPnpmSync(['config', 'get', 'user-agent'])

  t.equal(result.status, 0, 'command was successfull')
  t.ok(result.stdout.toString().includes('npm/'), 'command returned correct result')

  t.end()
})

test('pass through to npm with all the args', async t => {
  prepare(t)
  await rimraf('package.json')

  const result = execPnpmSync(['init', '-y'])

  t.equal(result.status, 0, 'command was successfull')
})

test('pnpm fails when an unsupported command is used', async (t) => {
  prepare(t)

  const { status, stdout } = execPnpmSync(['unsupported-command'])

  t.equal(status, 1, 'command failed')
  t.ok(stdout.toString().includes('Missing script: unsupported-command'))
})

test('pnpm fails when no command is specified', async (t) => {
  prepare(t)

  const { status, stdout } = execPnpmSync([])

  t.equal(status, 1, 'command failed')
  t.ok(stdout.toString().includes('Usage:'))
})

test('command fails when an unsupported flag is used', async (t) => {
  prepare(t)

  const { status, stderr } = execPnpmSync(['update', '--save-dev'])

  t.equal(status, 1, 'command failed')
  t.ok(stderr.toString().includes("Unknown option: 'save-dev'"))
})

test('command does not fail when a deprecated option is used', async (t) => {
  prepare(t)

  const { status, stdout } = execPnpmSync(['install', '--no-lock'])

  t.equal(status, 0, 'command did not fail')
  t.ok(stdout.toString().includes("Deprecated option: 'lock'"))
})

test('command does not fail when deprecated options are used', async (t) => {
  prepare(t)

  const { status, stdout } = execPnpmSync(['install', '--no-lock', '--independent-leaves'])

  t.equal(status, 0, 'command did not fail')
  t.ok(stdout.toString().includes("Deprecated options: 'lock', 'independent-leaves'"))
})

test('adding new dep does not fail if node_modules was created with --public-hoist-pattern=eslint-*', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm(['add', 'is-positive', '--public-hoist-pattern=eslint-*'])

  t.equal(execPnpmSync(['add', 'is-negative', '--no-hoist']).status, 1)
  t.equal(execPnpmSync(['add', 'is-negative', '--no-shamefully-hoist']).status, 1)
  t.equal(execPnpmSync(['add', 'is-negative']).status, 0)

  await project.has('is-negative')
})

test('pnpx works', t => {
  const result = execPnpxSync(['hello-world-js-bin'])

  t.equal(result.status, 0)
  t.ok(result.stdout.toString().includes('Hello world!'))

  t.end()
})

test('exit code from plugin is used to end the process', t => {
  process.chdir(hasOutdatedDepsFixture)
  const result = execPnpmSync(['outdated'])

  t.equal(result.status, 1)
  t.ok(result.stdout.toString().includes('is-positive'))

  t.end()
})
