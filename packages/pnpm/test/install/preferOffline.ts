import { WANTED_LOCKFILE } from '@pnpm/constants'
import prepare from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  execPnpm,
} from '../utils'
import rimraf = require('@zkochan/rimraf')
import tape = require('tape')

const test = promisifyTape(tape)

test('when prefer offline is used, meta from store is used, where latest might be out-of-date', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('foo', '100.0.0', 'latest')

  // This will cache the meta of `foo`
  await execPnpm(['install', 'foo'])

  await rimraf('node_modules')
  await rimraf(WANTED_LOCKFILE)

  await addDistTag('foo', '100.1.0', 'latest')

  await execPnpm(['install', 'foo', '--prefer-offline'])

  t.equal(project.requireModule('foo/package.json').version, '100.0.0')
})
