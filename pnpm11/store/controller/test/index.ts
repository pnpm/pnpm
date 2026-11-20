/// <reference path="../../../__typings__/index.d.ts"/>
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { createClient } from '@pnpm/installing.client'
import { createPackageStore } from '@pnpm/store.controller'
import type { FetchPackageToStoreFunction } from '@pnpm/store.controller-types'
import { StoreIndex } from '@pnpm/store.index'
import { temporaryDirectory } from 'tempy'

describe('store.importPackage()', () => {
  it('selects import method automatically', async () => {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const cacheDir = path.join(tmp, 'cache')
    const registry = 'https://registry.npmjs.org/'
    const storeIndex = new StoreIndex(storeDir)
    const { resolve, fetchers, clearResolutionCache } = createClient({
      configByUri: {},
      cacheDir: path.join(tmp, 'cache'),
      storeDir: path.join(tmp, 'store'),
      storeIndex,
      registriesByScope: {
        default: registry,
      },
    })
    const storeController = createPackageStore(resolve, fetchers, {
      storeDir,
      cacheDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
      clearResolutionCache,
      storeIndex,
    })
    const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
    const fetchResponse = (storeController.fetchPackage as FetchPackageToStoreFunction)({
      force: false,
      lockfileDir: temporaryDirectory(),
      pkg: {
        id: pkgId,
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
      },
    })
    const importTo = temporaryDirectory()
    const { importMethod } = await storeController.importPackage(importTo, {
      filesResponse: (await fetchResponse.fetching()).files,
      force: false,
    })
    expect(typeof importMethod).toBe('string')
    expect(typeof (await import(importTo)).default).toBe('function')
  })

  it('uses copying', async () => {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const cacheDir = path.join(tmp, 'cache')
    const registry = 'https://registry.npmjs.org/'
    const storeIndex = new StoreIndex(storeDir)
    const { resolve, fetchers, clearResolutionCache } = createClient({
      configByUri: {},
      cacheDir: path.join(tmp, 'cache'),
      storeDir: path.join(tmp, 'store'),
      storeIndex,
      registriesByScope: {
        default: registry,
      },
    })
    const storeController = createPackageStore(resolve, fetchers, {
      packageImportMethod: 'copy',
      storeDir,
      cacheDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
      clearResolutionCache,
      storeIndex,
    })
    const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
    const fetchResponse = (storeController.fetchPackage as FetchPackageToStoreFunction)({
      force: false,
      lockfileDir: temporaryDirectory(),
      pkg: {
        id: pkgId,
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
      },
    })
    const importTo = temporaryDirectory()
    const { importMethod } = await storeController.importPackage(importTo, {
      filesResponse: (await fetchResponse.fetching()).files,
      force: false,
    })
    expect(importMethod).toBe('copy')
    expect(typeof (await import(importTo)).default).toBe('function')
  })
})

describe('store.addFileToStore', () => {
  function packageStore (frozenStore: boolean) {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const storeIndex = new StoreIndex(storeDir)
    fs.mkdirSync(path.join(storeDir, 'files'), { recursive: true })
    return createPackageStore({} as never, {} as never, {
      storeDir,
      cacheDir: path.join(tmp, 'cache'),
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
      clearResolutionCache: () => {},
      frozenStore,
      storeIndex,
    })
  }

  it('is offered by a writable store', () => {
    expect(typeof packageStore(false).addFileToStore).toBe('function')
  })

  it('is withheld by a read-only store', () => {
    expect(packageStore(true).addFileToStore).toBeUndefined()
  })
})

describe('store.locateFileInStore', () => {
  function packageStore (verifyStoreIntegrity: boolean) {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const storeIndex = new StoreIndex(storeDir)
    fs.mkdirSync(path.join(storeDir, 'files'), { recursive: true })
    return createPackageStore({} as never, {} as never, {
      storeDir,
      cacheDir: path.join(tmp, 'cache'),
      verifyStoreIntegrity,
      virtualStoreDirMaxLength: 120,
      clearResolutionCache: () => {},
      storeIndex,
    })
  }

  it('offers content the store holds and nothing it does not', async () => {
    const store = packageStore(false)
    const bytes = Buffer.from('addon')
    const digest = createHash('sha512').update(bytes).digest('hex')
    expect(await store.locateFileInStore!(digest, 0o644)).toBeUndefined()

    const { filePath } = store.addFileToStore!(bytes, 0o644)
    expect(await store.locateFileInStore!(digest, 0o644)).toBe(filePath)
    // The store keeps executable content apart, so the same bytes under
    // another mode are a different file it does not yet have.
    expect(await store.locateFileInStore!(digest, 0o755)).toBeUndefined()
  })

  it.each([true, false])(
    'withholds content that no longer hashes to its digest (verifyStoreIntegrity: %s)',
    async (verifyStoreIntegrity) => {
      const store = packageStore(verifyStoreIntegrity)
      const bytes = Buffer.from('addon')
      const digest = createHash('sha512').update(bytes).digest('hex')
      const { filePath } = store.addFileToStore!(bytes, 0o644)
      expect(await store.locateFileInStore!(digest, 0o644)).toBe(filePath)

      fs.writeFileSync(filePath, 'tampered')
      expect(await store.locateFileInStore!(digest, 0o644)).toBeUndefined()
    }
  )
})

describe('remote side-effects metadata', () => {
  it('persists diffs and bounds quarantine entries', () => {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const storeIndex = new StoreIndex(storeDir)
    fs.mkdirSync(path.join(storeDir, 'files'), { recursive: true })
    const store = createPackageStore({} as never, {} as never, {
      storeDir,
      cacheDir: path.join(tmp, 'cache'),
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
      clearResolutionCache: () => {},
      storeIndex,
    })
    storeIndex.set('row', { algo: 'sha512', files: new Map() })

    expect(store.persistRemoteSideEffects?.({
      filesIndexFile: 'row',
      sideEffectsCacheKey: 'linux',
      sideEffects: { added: new Map(), deleted: [] },
    })).toBe(true)
    for (let index = 0; index < 70; index++) {
      store.quarantineRemoteSideEffects?.({
        channel: 'https://pnpr.example/',
        envelopeDigest: String(index).padStart(64, '0'),
        filesIndexFile: 'row',
      })
    }

    const row = storeIndex.get('row') as {
      sideEffects: Map<string, unknown>
      remoteSideEffectsQuarantine: Map<string, string[]>
    }
    expect(row.sideEffects.has('linux')).toBe(true)
    expect(row.remoteSideEffectsQuarantine.get('https://pnpr.example/')).toEqual(
      Array.from({ length: 64 }, (_, index) => String(index + 6).padStart(64, '0'))
    )
  })
})
