import { prepareEmpty } from '@pnpm/prepare'
import test from 'jest-t-assert'
import { addDependenciesToPackage } from 'supi'
import { testDefaults } from '../utils'

test('tarball from npm registry', async t => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['http://localhost:4873/is-array/-/is-array-1.0.1.tgz'], await testDefaults())

  const m = project.requireModule('is-array')

  t.ok(m, 'isArray() is available')

  await project.storeHas('localhost+4873/is-array/1.0.1')

  t.deepEqual(manifest.dependencies, { 'is-array': 'http://localhost:4873/is-array/-/is-array-1.0.1.tgz' }, 'has been added to dependencies in package.json')
})

test('tarball not from npm registry', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['https://github.com/hegemonic/taffydb/tarball/master'], await testDefaults())

  const m = project.requireModule('taffydb')

  t.ok(m, 'taffydb() is available')

  await project.storeHas('github.com/hegemonic/taffydb/tarball/master')
})

test('tarballs from GitHub (is-negative)', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['is-negative@https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz'], await testDefaults())

  const m = project.requireModule('is-negative')

  t.ok(m, 'isNegative() is available')
})
