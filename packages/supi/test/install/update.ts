import prepare from '@pnpm/prepare'
import { Shrinkwrap } from '@pnpm/shrinkwrap-file'
import path = require('path')
import readYamlFile from 'read-yaml-file'
import { addDependenciesToPackage, install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('preserve subdeps on update', async (t: tape.Test) => {
  const project = prepare(t)

  await Promise.all([
    addDistTag('abc-grand-parent-with-c', '1.0.0', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.0', 'latest'),
    addDistTag('bar', '100.0.0', 'latest'),
    addDistTag('foo', '100.0.0', 'latest'),
    addDistTag('foobarqar', '1.0.0', 'latest'),
    addDistTag('peer-c', '1.0.0', 'latest'),
  ])

  await addDependenciesToPackage(['foobarqar', 'abc-grand-parent-with-c'], await testDefaults())

  await Promise.all([
    addDistTag('abc-grand-parent-with-c', '1.0.1', 'latest'),
    addDistTag('abc-parent-with-ab', '1.0.1', 'latest'),
    addDistTag('bar', '100.1.0', 'latest'),
    addDistTag('foo', '100.1.0', 'latest'),
    addDistTag('foobarqar', '1.0.1', 'latest'),
  ])

  await install(await testDefaults({ update: true, depth: 0 }))

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

  await addDependenciesToPackage(['has-pkg-with-peer-only'], await testDefaults())

  await install(await testDefaults({ update: true, depth: Infinity }))

  t.pass('did not fail')
})

test('update does not install the package if it is not present in package.json', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['is-positive'], await testDefaults({
    allowNew: false,
    update: true,
  }))

  await project.hasNot('is-positive')
})

test('update dependency when external shrinkwrap directory is used', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('foo', '100.0.0', 'latest')

  const shrinkwrapDirectory = path.resolve('..')
  await addDependenciesToPackage(['foo'], await testDefaults({ shrinkwrapDirectory }))

  await addDistTag('foo', '100.1.0', 'latest')

  await install(await testDefaults({ update: true, depth: 0, shrinkwrapDirectory }))

  const shr = await readYamlFile<Shrinkwrap>(path.join('..', 'shrinkwrap.yaml'))

  t.ok(shr.packages && shr.packages['/foo/100.1.0']) // tslint:disable-line:no-string-literal
})
