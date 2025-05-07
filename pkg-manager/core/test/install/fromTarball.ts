import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils'

test('tarball from npm registry', async () => {
  const project = prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, [`http://localhost:${REGISTRY_MOCK_PORT}/is-array/-/is-array-1.0.1.tgz`], testDefaults())

  project.has('is-array')
  project.storeHas(`localhost+${REGISTRY_MOCK_PORT}/is-array/1.0.1`)

  expect(manifest.dependencies).toStrictEqual({ 'is-array': `http://localhost:${REGISTRY_MOCK_PORT}/is-array/-/is-array-1.0.1.tgz` })
})

test('tarball not from npm registry', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['https://github.com/hegemonic/taffydb/tarball/master'], testDefaults())

  project.has('taffydb')
  project.storeHas('github.com/hegemonic/taffydb/tarball/master')
})

test('tarballs from GitHub (is-negative)', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['is-negative@https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz'], testDefaults())

  project.has('is-negative')
})
