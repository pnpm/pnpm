import { prepareEmpty } from '@pnpm/prepare'
import test from 'jest-t-assert'
import rimraf = require('rimraf-then')
import { addDependenciesToPackage, storeStatus } from 'supi'
import { testDefaults } from './utils'

test('store status returns empty array when store was not modified', async t => {
  prepareEmpty(t)

  const opts = await testDefaults()
  await addDependenciesToPackage({}, ['is-positive@3.1.0'], opts)

  const mutatedPkgs = await storeStatus(opts)

  t.equal(mutatedPkgs && mutatedPkgs.length, 0, 'no packages were modified')
})

test('store status does not fail on not installed optional dependencies', async t => {
  prepareEmpty(t)

  const opts = await testDefaults({ targetDependenciesField: 'optionalDependencies' })
  await addDependenciesToPackage({}, ['not-compatible-with-any-os'], opts)

  const mutatedPkgs = await storeStatus(opts)

  t.equal(mutatedPkgs && mutatedPkgs.length, 0, 'no packages were modified')
})

test('store status returns path to the modified package', async t => {
  const project = prepareEmpty(t)

  const opts = await testDefaults()
  await addDependenciesToPackage({}, ['is-positive@3.1.0'], opts)

  const isPositive = await project.resolve('is-positive', '3.1.0', 'index.js')
  await rimraf(isPositive)

  const mutatedPkgs = await storeStatus(opts)

  t.equal(mutatedPkgs && mutatedPkgs.length, 1, '1 package was modified')
  t.ok(mutatedPkgs && mutatedPkgs[0].includes('is-positive'), 'is-positive was modified')
})
