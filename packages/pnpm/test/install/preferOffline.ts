import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  prepare,
  execPnpm,
} from '../utils'
import rimraf = require('rimraf-then')

const test = promisifyTape(tape)

test('when prefer offline is used, meta from store is used, where latest might be out-of-date', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('foo', '100.0.0', 'latest')

  // This will cache the meta of `foo`
  await execPnpm('install', 'foo')

  await rimraf('node_modules')
  await rimraf('shrinkwrap.yaml')

  await addDistTag('foo', '100.1.0', 'latest')

  await execPnpm('install', 'foo', '--prefer-offline')

  t.equal(project.requireModule('foo/package.json').version, '100.0.0')
})
