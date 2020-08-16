///<reference path="../../../typings/index.d.ts"/>
import createClient from '@pnpm/client'
import createStore from '@pnpm/package-store'
import path = require('path')
import test = require('tape')
import tempy = require('tempy')
import './createImportPackage.spec'

test('store.importPackage()', async (t) => {
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
  await storeController.importPackage(importTo, {
    filesResponse: await fetchResponse.files(),
    force: false,
  })
  t.equal(typeof require(importTo), 'function', `sucessfully imported to ${importTo}`)
  t.end()
})

test('store.importPackage() by copying', async (t) => {
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
  await storeController.importPackage(importTo, {
    filesResponse: await fetchResponse.files(),
    force: false,
  })
  t.equal(typeof require(importTo), 'function', `sucessfully imported to ${importTo}`)
  t.end()
})
