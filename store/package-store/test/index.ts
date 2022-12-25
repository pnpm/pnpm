/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { createClient } from '@pnpm/client'
import { createPackageStore } from '@pnpm/package-store'
import tempy from 'tempy'

describe('store.importPackage()', () => {
  it('selects import method automatically', async () => {
    const tmp = tempy.directory()
    const storeDir = path.join(tmp, 'store')
    const cacheDir = path.join(tmp, 'cache')
    const registry = 'https://registry.npmjs.org/'
    const authConfig = { registry }
    const { resolve, fetchers } = createClient({
      authConfig,
      cacheDir: path.join(tmp, 'cache'),
      rawConfig: {},
    })
    const storeController = await createPackageStore(resolve, fetchers, {
      storeDir,
      cacheDir,
      verifyStoreIntegrity: true,
    })
    const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
    const fetchResponse = storeController.fetchPackage({
      force: false,
      lockfileDir: tempy.directory(),
      pkg: {
        id: pkgId,
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          registry: 'https://registry.npmjs.org/',
          tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
      },
    })
    const importTo = tempy.directory()
    const { importMethod } = await storeController.importPackage(importTo, {
      filesResponse: await fetchResponse.files(),
      force: false,
    })
    expect(typeof importMethod).toBe('string')
    expect(typeof (await import(importTo)).default).toBe('function')
  })

  it('uses copying', async () => {
    const tmp = tempy.directory()
    const storeDir = path.join(tmp, 'store')
    const cacheDir = path.join(tmp, 'cache')
    const registry = 'https://registry.npmjs.org/'
    const authConfig = { registry }
    const { resolve, fetchers } = createClient({
      authConfig,
      cacheDir: path.join(tmp, 'cache'),
      rawConfig: {},
    })
    const storeController = await createPackageStore(resolve, fetchers, {
      packageImportMethod: 'copy',
      storeDir,
      cacheDir,
      verifyStoreIntegrity: true,
    })
    const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
    const fetchResponse = storeController.fetchPackage({
      force: false,
      lockfileDir: tempy.directory(),
      pkg: {
        id: pkgId,
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          registry: 'https://registry.npmjs.org/',
          tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
      },
    })
    const importTo = tempy.directory()
    const { importMethod } = await storeController.importPackage(importTo, {
      filesResponse: await fetchResponse.files(),
      force: false,
    })
    expect(importMethod).toBe('copy')
    expect(typeof (await import(importTo)).default).toBe('function')
  })
})
