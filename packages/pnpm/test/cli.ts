import prepare from '@pnpm/prepare'
import {
  execPnpm,
  execPnpmSync,
  execPnpxSync,
} from './utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import execa = require('execa')
import fs = require('mz/fs')

const fixtures = path.join(__dirname, '../../../fixtures')
const hasOutdatedDepsFixture = path.join(fixtures, 'has-outdated-deps')

test('some commands pass through to npm', () => {
  const result = execPnpmSync(['dist-tag', 'ls', 'is-positive'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).not.toMatch(/Usage: pnpm [command] [flags]/)
})

test('installs in the folder where the package.json file is', async () => {
  const project = prepare()

  await fs.mkdir('subdir')
  process.chdir('subdir')

  await execPnpm(['install', 'rimraf@2.5.1'])

  const m = project.requireModule('rimraf')
  expect(typeof m).toBe('function')
  await project.isExecutable('.bin/rimraf')
})

test('pnpm import does not move modules created by npm', async () => {
  prepare()

  await execa('npm', ['install', 'is-positive@1.0.0', '--save'])
  await execa('npm', ['shrinkwrap'])

  const packageManifestInodeBefore = (await fs.stat('node_modules/is-positive/package.json')).ino

  await execPnpm(['import'])

  const packageManifestInodeAfter = (await fs.stat('node_modules/is-positive/package.json')).ino

  expect(packageManifestInodeBefore).toBe(packageManifestInodeAfter)
})

test('pass through to npm CLI for commands that are not supported by npm', () => {
  const result = execPnpmSync(['config', 'get', 'user-agent'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toMatch(/npm\//) // command returned correct result
})

test('pass through to npm with all the args', async () => {
  prepare()
  await rimraf('package.json')

  const result = execPnpmSync(['init', '-y'])

  expect(result.status).toBe(0)
})

test('pnpm fails when an unsupported command is used', async () => {
  prepare()

  const { status, stdout } = execPnpmSync(['unsupported-command'])

  expect(status).toBe(1)
  expect(stdout.toString()).toMatch(/Missing script: unsupported-command/)
})

test('pnpm fails when no command is specified', async () => {
  prepare()

  const { status, stdout } = execPnpmSync([])

  expect(status).toBe(1)
  expect(stdout.toString()).toMatch(/Usage:/)
})

test('command fails when an unsupported flag is used', async () => {
  prepare()

  const { status, stderr } = execPnpmSync(['update', '--save-dev'])

  expect(status).toBe(1)
  expect(stderr.toString()).toMatch(/Unknown option: 'save-dev'/)
})

test('command does not fail when a deprecated option is used', async () => {
  prepare()

  const { status, stdout } = execPnpmSync(['install', '--no-lock'])

  expect(status).toBe(0)
  expect(stdout.toString()).toMatch(/Deprecated option: 'lock'/)
})

test('command does not fail when deprecated options are used', async () => {
  prepare()

  const { status, stdout } = execPnpmSync(['install', '--no-lock', '--independent-leaves'])

  expect(status).toBe(0)
  expect(stdout.toString()).toMatch(/Deprecated options: 'lock', 'independent-leaves'/)
})

test('adding new dep does not fail if node_modules was created with --public-hoist-pattern=eslint-*', async () => {
  const project = prepare()

  await execPnpm(['add', 'is-positive', '--public-hoist-pattern=eslint-*'])

  expect(execPnpmSync(['add', 'is-negative', '--no-hoist']).status).toBe(1)
  expect(execPnpmSync(['add', 'is-negative', '--no-shamefully-hoist']).status).toBe(1)
  expect(execPnpmSync(['add', 'is-negative']).status).toBe(0)

  await project.has('is-negative')
})

test('pnpx works', () => {
  const result = execPnpxSync(['hello-world-js-bin'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toMatch(/Hello world!/)
})

test('exit code from plugin is used to end the process', () => {
  process.chdir(hasOutdatedDepsFixture)
  const result = execPnpmSync(['outdated'])

  expect(result.status).toBe(1)
  expect(result.stdout.toString()).toMatch(/is-positive/)
})
