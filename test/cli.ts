import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import spawn = require('cross-spawn')
import exists = require('exists-file')
import {add as addDistTag} from './support/distTags'
import prepare from './support/prepare'
import runCli from './support/run-cli'

const pnpmBin = path.join(__dirname, '../src/bin/pnpm.ts')

test('return error status code when underlying command fails', t => {
  const result = spawn.sync('ts-node', [pnpmBin, 'invalid-command'])

  t.equal(result.status, 1, 'error status code returned')

  t.end()
})

test('update', async function (t) {
  const project = prepare(t)

  const latest = 'stable'

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', latest)

  await runCli('install', 'pkg-with-1-dep', '-S', '--tag', latest, '--cache-ttl', '0')

  project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', latest)

  await runCli('update', '--depth', '1', '--tag', latest)

  project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})
