import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'
import tape = require('tape')

const test = promisifyTape(tape)

test('fail if installed package does not support the current engine and engine-strict = true', async (t) => {
  const project = prepareEmpty(t)

  try {
    await addDependenciesToPackage({}, ['not-compatible-with-any-os'], await testDefaults({
      engineStrict: true,
    }))
    t.fail()
  } catch (err) {
    await project.hasNot('not-compatible-with-any-os')
    await project.storeHasNot('not-compatible-with-any-os', '1.0.0')
  }
})

test('do not fail if installed package does not support the current engine and engine-strict = false', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['not-compatible-with-any-os'], await testDefaults({
    engineStrict: false,
  }))

  await project.has('not-compatible-with-any-os')
  await project.storeHas('not-compatible-with-any-os', '1.0.0')

  const lockfile = await project.readLockfile()
  t.deepEqual(lockfile.packages['/not-compatible-with-any-os/1.0.0'].os, ['this-os-does-not-exist'], `os field added to ${WANTED_LOCKFILE}`)
})

test('do not fail if installed package requires the node version that was passed in and engine-strict = true', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['for-legacy-node'], await testDefaults({
    engineStrict: true,
    nodeVersion: '0.10.0',
  }))

  await project.has('for-legacy-node')
  await project.storeHas('for-legacy-node', '1.0.0')

  const lockfile = await project.readLockfile()
  t.deepEqual(lockfile.packages['/for-legacy-node/1.0.0'].engines, { node: '0.10' }, `engines field added to ${WANTED_LOCKFILE}`)
})

test(`save cpu field to ${WANTED_LOCKFILE}`, async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['has-cpu-specified'], await testDefaults())

  const lockfile = await project.readLockfile()

  t.deepEqual(
    lockfile.packages['/has-cpu-specified/1.0.0'].cpu,
    ['x64', 'ia32'],
    `cpu field added to ${WANTED_LOCKFILE}`
  )
})

test(`engines field is not added to ${WANTED_LOCKFILE} when "node": "*" is in "engines" field`, async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['jsonify@0.0.0'], await testDefaults())

  const lockfile = await project.readLockfile()

  t.notOk(
    lockfile.packages['/jsonify/0.0.0'].engines,
    `engines field is not added to ${WANTED_LOCKFILE}`
  )
})
