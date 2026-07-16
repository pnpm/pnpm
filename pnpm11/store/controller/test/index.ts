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

const builtInImporters = [
  ['synchronous', (storeDir: string) => createCafsStore(storeDir, { packageImportMethod: 'hardlink' }).importPackage],
  ['asynchronous', (storeDir: string) => createPackageImporterAsync({ storeDir, packageImportMethod: 'hardlink' })],
] as const

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

  it.each(builtInImporters)('makes private build projections writable without changing read-only store files using the %s importer', async (_name, createImporter) => {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const filesMap = new Map([
      ['index.js', path.join(storeDir, 'index.js')],
      ['node_modules/bundled/index.js', path.join(storeDir, 'bundled-index.js')],
      ['package.json', path.join(storeDir, 'package.json')],
    ])
    fs.mkdirSync(storeDir, { recursive: true })
    fs.writeFileSync(filesMap.get('index.js')!, 'module.exports = true')
    fs.writeFileSync(filesMap.get('node_modules/bundled/index.js')!, 'module.exports = "bundled"')
    fs.writeFileSync(filesMap.get('package.json')!, '{"name":"fixture"}')
    fs.chmodSync(filesMap.get('index.js')!, 0o555)
    fs.chmodSync(filesMap.get('node_modules/bundled/index.js')!, 0o444)
    fs.chmodSync(filesMap.get('package.json')!, 0o444)

    const importPackage = createImporter(storeDir)
    const importTo = `${path.join(tmp, 'project', 'node_modules', 'fixture')}${path.sep}`
    const filesResponse = {
      filesMap,
      requiresBuild: true,
      resolvedFrom: 'store' as const,
    }
    await importPackage(importTo, {
      filesResponse,
      force: false,
      requiresBuild: false,
    })
    if (process.platform !== 'win32') {
      expect(fs.statSync(path.join(importTo, 'package.json')).ino).toBe(fs.statSync(filesMap.get('package.json')!).ino)
    }
    const nestedDependency = path.join(importTo, 'node_modules', 'nested', 'index.js')
    fs.mkdirSync(path.dirname(nestedDependency), { recursive: true })
    fs.writeFileSync(nestedDependency, 'module.exports = true')
    fs.chmodSync(nestedDependency, 0o444)
    fs.unlinkSync(path.join(importTo, 'index.js'))

    await importPackage(importTo, {
      filesResponse,
      force: false,
      keepModulesDir: true,
      requiresBuild: true,
      safeToSkip: true,
    })

    expect(fs.statSync(path.join(importTo, 'index.js')).mode & 0o200).toBe(0o200)
    expect(fs.statSync(path.join(importTo, 'node_modules/bundled/index.js')).mode & 0o200).toBe(0o200)
    expect(fs.statSync(path.join(importTo, 'package.json')).mode & 0o200).toBe(0o200)
    expect(fs.statSync(importTo).mode & 0o200).toBe(0o200)
    expect(fs.statSync(filesMap.get('index.js')!).mode & 0o200).toBe(0)
    expect(fs.statSync(filesMap.get('node_modules/bundled/index.js')!).mode & 0o200).toBe(0)
    expect(fs.statSync(filesMap.get('package.json')!).mode & 0o200).toBe(0)
    if (process.platform !== 'win32') {
      expect(fs.statSync(path.join(importTo, 'index.js')).mode & 0o777).toBe(0o755)
      expect(fs.statSync(path.join(importTo, 'package.json')).mode & 0o777).toBe(0o644)
      expect(fs.statSync(path.join(importTo, 'package.json')).ino).not.toBe(fs.statSync(filesMap.get('package.json')!).ino)
      expect(fs.statSync(nestedDependency).mode & 0o200).toBe(0)
    }
    fs.appendFileSync(path.join(importTo, 'index.js'), '\n// build output')
    fs.writeFileSync(path.join(importTo, 'generated.txt'), 'build output')
    expect(fs.readFileSync(filesMap.get('index.js')!, 'utf8')).toBe('module.exports = true')
    const projectedIndex = path.join(importTo, 'index.js')
    fs.chmodSync(projectedIndex, fs.statSync(projectedIndex).mode & ~0o200)

    await importPackage(importTo, {
      filesResponse,
      force: false,
      requiresBuild: true,
      safeToSkip: true,
    })
    expect(fs.readFileSync(path.join(importTo, 'index.js'), 'utf8')).toContain('// build output')
    expect(fs.readFileSync(path.join(importTo, 'generated.txt'), 'utf8')).toBe('build output')
    expect(fs.statSync(projectedIndex).mode & 0o200).toBe(0o200)

    const cachedImportTo = path.join(tmp, 'cached-project', 'node_modules', 'fixture')
    const cachedResult = await importPackage(cachedImportTo, {
      filesResponse: {
        ...filesResponse,
        sideEffectsMaps: new Map([['cached-build', {}]]),
      },
      force: false,
      requiresBuild: true,
      sideEffectsCacheKey: 'cached-build',
    })
    expect(cachedResult.isBuilt).toBe(true)
    expect(fs.statSync(path.join(cachedImportTo, 'package.json')).mode & 0o200).toBe(0o200)
    if (process.platform !== 'win32') {
      expect(fs.statSync(path.join(cachedImportTo, 'package.json')).ino).not.toBe(fs.statSync(filesMap.get('package.json')!).ino)
    }
  })

  it.each(builtInImporters)('replaces store hardlinks in partial projections using the %s importer', async (_name, createImporter) => {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const storeFile = path.join(storeDir, 'index.js')
    const storeManifest = path.join(storeDir, 'package.json')
    fs.mkdirSync(storeDir, { recursive: true })
    fs.writeFileSync(storeFile, 'module.exports = true')
    fs.writeFileSync(storeManifest, '{"name":"fixture"}')

    const importTo = path.join(tmp, 'project', 'node_modules', 'fixture')
    fs.mkdirSync(importTo, { recursive: true })
    fs.linkSync(storeFile, path.join(importTo, 'index.js'))
    fs.chmodSync(storeFile, 0o444)

    const importPackage = createImporter(storeDir)
    await importPackage(importTo, {
      filesResponse: {
        filesMap: new Map([
          ['index.js', storeFile],
          ['package.json', storeManifest],
        ]),
        requiresBuild: true,
        resolvedFrom: 'store',
      },
      force: false,
      requiresBuild: true,
      safeToSkip: true,
    })

    const projectedFile = path.join(importTo, 'index.js')
    expect(fs.statSync(storeFile).mode & 0o200).toBe(0)
    expect(fs.statSync(projectedFile).mode & 0o200).toBe(0o200)
    if (process.platform !== 'win32') {
      expect(fs.statSync(projectedFile).ino).not.toBe(fs.statSync(storeFile).ino)
    }
  })

  it.each(builtInImporters)('replaces a symlinked build projection without changing its target using the %s importer', async (_name, createImporter) => {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const storeManifest = path.join(storeDir, 'package.json')
    fs.mkdirSync(storeDir, { recursive: true })
    fs.writeFileSync(storeManifest, '{"name":"fixture"}')

    const externalDir = path.join(tmp, 'external')
    fs.mkdirSync(externalDir)
    fs.writeFileSync(path.join(externalDir, 'package.json'), '{"name":"external"}')
    const importTo = path.join(tmp, 'project', 'node_modules', 'fixture')
    fs.mkdirSync(path.dirname(importTo), { recursive: true })
    fs.symlinkSync(externalDir, importTo, process.platform === 'win32' ? 'junction' : 'dir')

    const importPackage = createImporter(storeDir)
    await importPackage(importTo, {
      filesResponse: {
        filesMap: new Map([['package.json', storeManifest]]),
        requiresBuild: true,
        resolvedFrom: 'store',
      },
      force: false,
      requiresBuild: true,
    })

    expect(fs.lstatSync(importTo).isDirectory()).toBe(true)
    expect(fs.readFileSync(path.join(importTo, 'package.json'), 'utf8')).toBe('{"name":"fixture"}')
    expect(fs.readFileSync(path.join(externalDir, 'package.json'), 'utf8')).toBe('{"name":"external"}')
  })

  it.each(builtInImporters)('replaces a symlinked package file without changing its target using the %s importer', async (_name, createImporter) => {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const storeFile = path.join(storeDir, 'index.js')
    const storeManifest = path.join(storeDir, 'package.json')
    fs.mkdirSync(storeDir, { recursive: true })
    fs.writeFileSync(storeFile, 'module.exports = "store"')
    fs.writeFileSync(storeManifest, '{"name":"fixture"}')

    const externalFile = path.join(tmp, 'external.js')
    fs.writeFileSync(externalFile, 'module.exports = "external"')
    const importTo = path.join(tmp, 'project', 'node_modules', 'fixture')
    fs.mkdirSync(importTo, { recursive: true })
    fs.copyFileSync(storeManifest, path.join(importTo, 'package.json'))
    fs.symlinkSync(externalFile, path.join(importTo, 'index.js'), 'file')

    const importPackage = createImporter(storeDir)
    await importPackage(importTo, {
      filesResponse: {
        filesMap: new Map([
          ['index.js', storeFile],
          ['package.json', storeManifest],
        ]),
        requiresBuild: true,
        resolvedFrom: 'store',
      },
      force: false,
      requiresBuild: true,
      safeToSkip: true,
    })

    expect(fs.lstatSync(path.join(importTo, 'index.js')).isFile()).toBe(true)
    expect(fs.readFileSync(path.join(importTo, 'index.js'), 'utf8')).toBe('module.exports = "store"')
    expect(fs.readFileSync(externalFile, 'utf8')).toBe('module.exports = "external"')
  })

  it.each(builtInImporters)('replaces store hardlinks from old entries without package.json before making a package writable using the %s importer', async (_name, createImporter) => {
    const tmp = temporaryDirectory()
    const storeDir = path.join(tmp, 'store')
    const storeInvalidFile = path.join(storeDir, 'invalid.js')
    fs.mkdirSync(storeDir, { recursive: true })
    fs.writeFileSync(storeInvalidFile, 'module.exports = true')

    const importTo = path.join(tmp, 'project', 'node_modules', 'fixture')
    fs.mkdirSync(importTo, { recursive: true })
    fs.linkSync(storeInvalidFile, path.join(importTo, 'filename.js'))
    fs.chmodSync(storeInvalidFile, 0o444)

    const importPackage = createImporter(storeDir)
    await importPackage(importTo, {
      filesResponse: {
        filesMap: new Map([['filename.js', storeInvalidFile]]),
        requiresBuild: true,
        resolvedFrom: 'store',
      },
      force: false,
      requiresBuild: true,
      safeToSkip: true,
    })

    const projectedInvalidFile = path.join(importTo, 'filename.js')
    expect(fs.statSync(storeInvalidFile).mode & 0o200).toBe(0)
    expect(fs.statSync(projectedInvalidFile).mode & 0o200).toBe(0o200)
    if (process.platform !== 'win32') {
      expect(fs.statSync(projectedInvalidFile).ino).not.toBe(fs.statSync(storeInvalidFile).ino)
    }
  })

  it('does not chmod output owned by a custom importPackage hook', async () => {
    const tmp = temporaryDirectory()
    const storeFile = path.join(tmp, 'store', 'package.json')
    fs.mkdirSync(path.dirname(storeFile), { recursive: true })
    fs.writeFileSync(storeFile, '{"name":"fixture"}')
    fs.chmodSync(storeFile, 0o444)
    const importTo = path.join(tmp, 'project', 'node_modules', 'fixture')
    const importPackage = createPackageImporterAsync({
      storeDir: path.join(tmp, 'store'),
      importIndexedPackage: async (to, opts) => {
        fs.mkdirSync(to, { recursive: true })
        fs.linkSync(opts.filesMap.get('package.json')!, path.join(to, 'package.json'))
        return 'hardlink'
      },
    })

    await importPackage(importTo, {
      filesResponse: {
        filesMap: new Map([['package.json', storeFile]]),
        requiresBuild: true,
        resolvedFrom: 'store',
      },
      force: false,
      requiresBuild: true,
    })

    expect(fs.statSync(storeFile).mode & 0o200).toBe(0)
    expect(fs.statSync(path.join(importTo, 'package.json')).mode & 0o200).toBe(0)
  })
})
