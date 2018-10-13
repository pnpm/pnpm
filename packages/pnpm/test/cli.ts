import prepare from '@pnpm/prepare'
import { fromDir as readPackage } from '@pnpm/read-package-json'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import execa = require('execa')
import mkdirp = require('mkdirp-promise')
import rimraf = require('rimraf-then')
import fs = require('mz/fs')
import {
  addDistTag,
  execPnpm,
  execPnpmSync,
} from './utils'

const test = promisifyTape(tape)

test('returns help when not available command is used', t => {
  const result = execPnpmSync('foobarqar')

  t.equal(result.status, 0)
  t.ok(result.stdout.toString().indexOf('Usage: pnpm [command] [flags]') !== -1)

  t.end()
})

test('some commands pass through to npm', t => {
  const result = execPnpmSync('dist-tag', 'ls', 'is-positive')

  t.equal(result.status, 0)
  t.ok(result.stdout.toString().indexOf('Usage: pnpm [command] [flags]') === -1)

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

test('pnpm import does not move modules created by npm', async (t: tape.Test) => {
  const project = prepare(t)

  await execa('npm', ['install', 'is-positive@1.0.0', '--save'])
  await execa('npm', ['shrinkwrap'])

  const packageJsonInodeBefore = (await fs.stat('node_modules/is-positive/package.json')).ino

  await execPnpm('import')

  const packageJsonInodeAfter = (await fs.stat('node_modules/is-positive/package.json')).ino

  t.equal(packageJsonInodeBefore, packageJsonInodeAfter)
})

test('update', async function (t: tape.Test) {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '101.0.0', 'latest')

  await execPnpm('install', 'dep-of-pkg-with-1-dep@100.0.0')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await execPnpm('update', 'dep-of-pkg-with-1-dep@latest')

  await project.storeHas('dep-of-pkg-with-1-dep', '101.0.0')

  const shr = await project.loadShrinkwrap()
  t.equal(shr.dependencies['dep-of-pkg-with-1-dep'], '101.0.0')

  const pkg = await readPackage(process.cwd())
  t.equal(pkg.dependencies && pkg.dependencies['dep-of-pkg-with-1-dep'], '^101.0.0')
})

test('deep update', async function (t: tape.Test) {
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
