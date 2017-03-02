import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import spawn = require('cross-spawn')
import exists = require('path-exists')
import {
  prepare,
  addDistTag,
  execPnpm,
  execPnpmSync,
} from './utils'

test('return error status code when underlying command fails', t => {
  const result = execPnpmSync('invalid-command')

  t.equal(result.status, 1, 'error status code returned')

  t.end()
})

test('update', async function (t) {
  const project = prepare(t)

  const latest = 'stable'

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', latest)

  await execPnpm('install', 'pkg-with-1-dep', '-S', '--tag', latest, '--cache-ttl', '0')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', latest)

  await execPnpm('update', '--depth', '1', '--tag', latest)

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})

test('installation via the CLI', async function (t) {
  const project = prepare(t)
  const result = execPnpmSync('install', 'rimraf@2.5.1')

  t.equal(result.status, 0, 'install successful')

  const rimraf = project.requireModule('rimraf')
  t.ok(typeof rimraf === 'function', 'rimraf() is available')

  await project.isExecutable('.bin/rimraf')
})

test('pass through to npm CLI for commands that are not supported by npm', t => {
  const result = execPnpmSync('config', 'get', 'user-agent')

  t.equal(result.status, 0, 'command was successfull')
  t.ok(result.stdout.toString().indexOf('npm/') !== -1, 'command returned correct result')

  t.end()
})
