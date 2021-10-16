import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from 'supi'
import rimraf from '@zkochan/rimraf'
import { testDefaults } from './utils'

test('offline installation fails when package meta not found in local registry mirror', async () => {
  prepareEmpty()

  try {
    await addDependenciesToPackage({}, ['is-positive@3.0.0'], await testDefaults({}, { offline: true }, { offline: true }))
    throw new Error('installation should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(err.code).toBe('ERR_PNPM_NO_OFFLINE_META')
  }
})

test('offline installation fails when package tarball not found in local registry mirror', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive@3.0.0'], await testDefaults())

  await rimraf('node_modules')

  try {
    await addDependenciesToPackage(manifest, ['is-positive@3.1.0'], await testDefaults({}, { offline: true }, { offline: true }))
    throw new Error('installation should have failed')
  } catch (err: any) { // eslint-disable-line
    expect(err.code).toBe('ERR_PNPM_NO_OFFLINE_TARBALL')
  }
})

test('successful offline installation', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['is-positive@3.0.0'], await testDefaults({ save: true }))

  await rimraf('node_modules')

  await install(manifest, await testDefaults({}, { offline: true }, { offline: true }))

  await project.has('is-positive')
})
