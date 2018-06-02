import {install, installPkgs} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('preserve subdeps on update', async (t: tape.Test) => {
  const project = prepare(t)

  await Promise.all([
    addDistTag('foobarqar', '1.0.0', 'latest'),
    addDistTag('foo', '100.0.0', 'latest'),
    addDistTag('bar', '100.0.0', 'latest'),
    addDistTag('abc-grand-parent-with-c', '1.0.0', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.0', 'latest'),
  ])

  await installPkgs(['foobarqar', 'abc-grand-parent-with-c'], await testDefaults())

  await Promise.all([
    addDistTag('foobarqar', '1.0.1', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('abc-grand-parent-with-c', '1.0.1', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.1', 'latest'),
  ])

  await install(await testDefaults({update: true, depth: 0}))

  const shr = await project.loadShrinkwrap()

  t.ok(shr.packages)
  t.ok(shr.packages['/abc-parent-with-ab/1.0.0/peer-c@1.0.0'], 'preserve version of package that has resolved peer deps')
  t.ok(shr.packages['/foobarqar/1.0.1'])
  t.deepEqual(shr.packages['/foobarqar/1.0.1'].dependencies, {
    bar: '100.0.0',
    foo: '100.0.0',
    qar: '100.0.0',
  })
})

test('update does not fail when package has only peer dependencies', async (t: tape.Test) => {
  prepare(t)

  await installPkgs(['has-pkg-with-peer-only'], await testDefaults())

  await install(await testDefaults({update: true, depth: Infinity}))

  t.pass('did not fail')
})

test('update does not install the package if it is not present in package.json', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-positive'], await testDefaults({
    allowNew: false,
    update: true,
  }))

  project.hasNot('is-positive')
})
