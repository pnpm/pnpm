/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { createClient } from '@pnpm/installing.client'
import { createPackageStore } from '@pnpm/store.controller'
import type { FetchPackageToStoreFunction } from '@pnpm/store.controller-types'
import { createCafsStore, createPackageImporterAsync } from '@pnpm/store.create-cafs-store'
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
      registries: {
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

  it.each([
    ['synchronous', (storeDir: string) => createCafsStore(storeDir, { packageImportMethod: 'hardlink' }).importPackage],
    ['asynchronous', (storeDir: string) => createPackageImporterAsync({ storeDir, packageImportMethod: 'hardlink' })],
  ] as const)('uses a private writable projection for a package that still needs building with the %s importer', async (_name, createImporter) => {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const storeManifest = path.join(storeDir, 'package.json')
    fs.mkdirSync(storeDir, { recursive: true })
    fs.writeFileSync(storeManifest, '{"name":"fixture"}')
    fs.chmodSync(storeManifest, 0o444)

    const importTo = path.join(tmp, 'project', 'node_modules', 'fixture')
    await createImporter(storeDir)(importTo, {
      filesResponse: {
        filesMap: new Map([['package.json', storeManifest]]),
        requiresBuild: true,
        resolvedFrom: 'store',
      },
      force: false,
      requiresBuild: true,
    })

    const projectedManifest = path.join(importTo, 'package.json')
    expect(fs.statSync(projectedManifest).mode & 0o200).toBe(0o200)
    expect(fs.statSync(storeManifest).mode & 0o200).toBe(0)
    if (process.platform !== 'win32') {
      expect(fs.statSync(projectedManifest).ino).not.toBe(fs.statSync(storeManifest).ino)
    }
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
      registries: {
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
