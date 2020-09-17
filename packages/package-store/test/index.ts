/// <reference path="../../../typings/index.d.ts"/>
import createClient from '@pnpm/client'
import createStore from '@pnpm/package-store'
import tempy = require('tempy')

describe('store.importPackage()', () => {
  it('selects import method automatically', async () => {
    const storeDir = tempy.directory()
    const registry = 'https://registry.npmjs.org/'
    const authConfig = { registry }
    const { resolve, fetchers } = createClient({
      authConfig,
      storeDir,
    })
    const storeController = await createStore(resolve, fetchers, {
      storeDir,
      verifyStoreIntegrity: true,
    })
    const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
    const fetchResponse = storeController.fetchPackage({
      force: false,
      lockfileDir: tempy.directory(),
      pkgId,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
    })
    const importTo = tempy.directory()
    const { importMethod } = await storeController.importPackage(importTo, {
      filesResponse: await fetchResponse.files(),
      force: false,
    })
    expect(typeof importMethod).toBe('string')
    expect(typeof require(importTo)).toBe('function')
  })

  it('uses copying', async () => {
    const storeDir = tempy.directory()
    const registry = 'https://registry.npmjs.org/'
    const authConfig = { registry }
    const { resolve, fetchers } = createClient({
      authConfig,
      storeDir,
    })
    const storeController = await createStore(resolve, fetchers, {
      packageImportMethod: 'copy',
      storeDir,
      verifyStoreIntegrity: true,
    })
    const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
    const fetchResponse = storeController.fetchPackage({
      force: false,
      lockfileDir: tempy.directory(),
      pkgId,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
    })
    const importTo = tempy.directory()
    const { importMethod } = await storeController.importPackage(importTo, {
      filesResponse: await fetchResponse.files(),
      force: false,
    })
    expect(importMethod).toBe('copy')
    expect(typeof require(importTo)).toBe('function')
  })
})
