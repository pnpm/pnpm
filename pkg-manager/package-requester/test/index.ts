/// <reference path="../../../__typings__/index.d.ts" />
import { promises as fs, statSync } from 'fs'
import path from 'path'
import { getFilePathInCafs, PackageFilesIndex } from '@pnpm/cafs'
import { createClient } from '@pnpm/client'
import { streamParser } from '@pnpm/logger'
import { createPackageRequester, PackageResponse } from '@pnpm/package-requester'
import type { PackageFilesResponse } from '@pnpm/cafs-types'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { DependencyManifest } from '@pnpm/types'
import delay from 'delay'
import { depPathToFilename } from 'dependency-path'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import normalize from 'normalize-path'
import tempy from 'tempy'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}`
const f = fixtures(__dirname)
const IS_POSTIVE_TARBALL = f.find('is-positive-1.0.0.tgz')

const authConfig = { registry }

const { resolve, fetchers } = createClient({
  authConfig,
  cacheDir: '.store',
})

test('request package', async () => {
  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })
  expect(typeof requestPackage).toBe('function')

  const projectDir = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
  })

  expect(pkgResponse).toBeTruthy()
  expect(pkgResponse.body).toBeTruthy()

  expect(pkgResponse.body.id).toBe(`localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`)
  expect(pkgResponse.body.resolvedVia).toBe('npm-registry')
  expect(pkgResponse.body.isLocal).toBe(false)
  expect(typeof pkgResponse.body.latest).toBe('string')
  expect(pkgResponse.body.manifest?.name).toBe('is-positive')
  expect(!pkgResponse.body.normalizedPref).toBeTruthy()
  expect(pkgResponse.body.resolution).toStrictEqual({
    integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
  })

  const files = await pkgResponse.files!()
  expect(Object.keys(files.filesIndex).sort()).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort())
  expect(files.fromStore).toBeFalsy()

  expect(pkgResponse.finishing!()).toBeTruthy()
})

test('request package but skip fetching', async () => {
  const storeDir = '.store'
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })
  expect(typeof requestPackage).toBe('function')

  const projectDir = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
    skipFetch: true,
  })

  expect(pkgResponse).toBeTruthy()
  expect(pkgResponse.body).toBeTruthy()

  expect(pkgResponse.body.id).toBe(`localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`)
  expect(pkgResponse.body.isLocal).toBe(false)
  expect(typeof pkgResponse.body.latest).toBe('string')
  expect(pkgResponse.body.manifest?.name).toBe('is-positive')
  expect(!pkgResponse.body.normalizedPref).toBeTruthy()
  expect(pkgResponse.body.resolution).toStrictEqual({
    integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
  })

  expect(pkgResponse.files).toBeFalsy()
  expect(pkgResponse.finishing).toBeFalsy()
})

test('request package but skip fetching, when resolution is already available', async () => {
  const storeDir = '.store'
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })
  expect(typeof requestPackage).toBe('function')

  const projectDir = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
    currentPkg: {
      id: `localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
    skipFetch: true,
    update: false,
  }) as PackageResponse & {
    body: {
      latest: string
      manifest: { name: string }
    }
    files: () => Promise<object>
    finishing: () => Promise<void>
  }

  expect(pkgResponse).toBeTruthy()
  expect(pkgResponse.body).toBeTruthy()

  expect(pkgResponse.body.id).toBe(`localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`)
  expect(pkgResponse.body.isLocal).toBe(false)
  expect(typeof pkgResponse.body.latest).toBe('string')
  expect(pkgResponse.body.manifest.name).toBe('is-positive')
  expect(!pkgResponse.body.normalizedPref).toBeTruthy()
  expect(pkgResponse.body.resolution).toStrictEqual({
    integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
    registry: `http://localhost:${REGISTRY_MOCK_PORT}`,
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
  })

  expect(pkgResponse.files).toBeFalsy()
  expect(pkgResponse.finishing).toBeFalsy()
})

test('refetch local tarball if its integrity has changed', async () => {
  const projectDir = tempy.directory()
  const tarballPath = path.join(projectDir, 'tarball.tgz')
  const tarballRelativePath = path.relative(projectDir, tarballPath)
  f.copy('pnpm-package-requester-0.8.1.tgz', tarballPath)
  const tarball = `file:${tarballRelativePath}`
  const wantedPackage = { pref: tarball }
  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)
  const pkgId = `file:${normalize(tarballRelativePath)}`
  const requestPackageOpts = {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
    skipFetch: true,
    update: false,
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      currentPkg: {
        id: pkgId,
        resolution: {
          integrity: 'sha512-lqODmYcc/FKOGROEUByd5Sbugqhzgkv+Hij9PXH0sZVQsU2npTQ0x3L81GCtHilFKme8lhBtD31Vxg/AKYrAvg==',
          tarball,
        },
      },
    }) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>
      finishing: () => Promise<void>
    }
    await response.files()
    await response.finishing()

    expect(response.body.updated).toBeFalsy()
    expect((await response.files()).fromStore).toBeFalsy()
    expect(await response.bundledManifest!()).toBeTruthy()
  }

  f.copy('pnpm-package-requester-4.1.2.tgz', tarballPath)
  await delay(50)

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      currentPkg: {
        id: pkgId,
        resolution: {
          integrity: 'sha512-lqODmYcc/FKOGROEUByd5Sbugqhzgkv+Hij9PXH0sZVQsU2npTQ0x3L81GCtHilFKme8lhBtD31Vxg/AKYrAvg==',
          tarball,
        },
      },
    })
    await response.files!()
    await response.finishing!()

    expect(response.body.updated).toBeTruthy()
    expect((await response.files!()).fromStore).toBeFalsy()
    expect(await response.bundledManifest!()).toBeTruthy()
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      currentPkg: {
        id: pkgId,
        resolution: {
          integrity: 'sha512-v3uhYkN+Eh3Nus4EZmegjQhrfpdPIH+2FjrkeBc6ueqZJWWRaLnSYIkD0An6m16D3v+6HCE18ox6t95eGxj5Pw==',
          tarball,
        },
      },
    }) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>
      finishing: () => Promise<void>
    }
    await response.files()
    await response.finishing()

    expect(response.body.updated).toBeFalsy()
    expect((await response.files()).fromStore).toBeTruthy()
    expect(await response.bundledManifest!()).toBeTruthy()
  }
})

test('refetch local tarball if its integrity has changed. The requester does not know the correct integrity', async () => {
  const projectDir = tempy.directory()
  const tarballPath = path.join(projectDir, 'tarball.tgz')
  f.copy('pnpm-package-requester-0.8.1.tgz', tarballPath)
  const tarball = `file:${tarballPath}`
  const wantedPackage = { pref: tarball }
  const storeDir = path.join(__dirname, '..', '.store')
  const cafs = createCafsStore(storeDir)
  const requestPackageOpts = {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
    update: false,
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>
      finishing: () => Promise<void>
    }
    await response.files()
    await response.finishing()

    expect(response.body.updated).toBeTruthy()
    expect((await response.files()).fromStore).toBeFalsy()
    expect(await response.bundledManifest!()).toBeTruthy()
  }

  f.copy('pnpm-package-requester-4.1.2.tgz', tarballPath)
  await delay(50)

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>
      finishing: () => Promise<void>
    }
    await response.files()
    await response.finishing()

    expect(response.body.updated).toBeTruthy()
    expect((await response.files()).fromStore).toBeFalsy()
    expect(await response.bundledManifest!()).toBeTruthy()
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>
      finishing: () => Promise<void>
    }
    await response.files()
    await response.finishing()

    expect((await response.files()).fromStore).toBeTruthy()
    expect(await response.bundledManifest!()).toBeTruthy()
  }
})

test('fetchPackageToStore()', async () => {
  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)
  const packageRequester = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })

  const pkgId = `localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`
  const fetchResult = packageRequester.fetchPackageToStore({
    force: false,
    lockfileDir: tempy.directory(),
    pkg: {
      name: 'is-positive',
      version: '1.0.0',
      id: pkgId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
  })

  expect(fetchResult.bundledManifest).toBeFalsy()

  const files = await fetchResult.files()
  expect(Object.keys(files.filesIndex).sort()).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort())
  expect(files.fromStore).toBeFalsy()

  const indexFile = await loadJsonFile<PackageFilesIndex>(fetchResult.filesIndexFile)
  expect(indexFile).toBeTruthy()
  expect(typeof indexFile.files['package.json'].checkedAt).toBeTruthy()

  expect(fetchResult.finishing()).toBeTruthy()

  const fetchResult2 = packageRequester.fetchPackageToStore({
    fetchRawManifest: true,
    force: false,
    lockfileDir: tempy.directory(),
    pkg: {
      name: 'is-positive',
      version: '1.0.0',
      id: pkgId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
  })

  // This verifies that when a package has been cached with no full manifest
  // the full manifest is requested and added to the cache
  expect(
    await fetchResult2.bundledManifest!()
  ).toStrictEqual(
    {
      engines: { node: '>=0.10.0' },
      name: 'is-positive',
      scripts: { test: 'node test.js' },
      version: '1.0.0',
    }
  )
})

test('fetchPackageToStore() concurrency check', async () => {
  const storeDir = tempy.directory()
  const cafsDir = path.join(storeDir, 'files')
  const cafs = createCafsStore(storeDir)
  const packageRequester = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })

  const pkgId = `localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`
  const projectDir1 = tempy.directory()
  const projectDir2 = tempy.directory()
  const fetchResults = await Promise.all([
    packageRequester.fetchPackageToStore({
      force: false,
      lockfileDir: projectDir1,
      pkg: {
        name: 'is-positive',
        version: '1.0.0',
        id: pkgId,
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
        },
      },
    }),
    packageRequester.fetchPackageToStore({
      force: false,
      lockfileDir: projectDir2,
      pkg: {
        name: 'is-positive',
        version: '1.0.0',
        id: pkgId,
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
        },
      },
    }),
  ])

  let ino1!: Number
  let ino2!: Number

  {
    const fetchResult = fetchResults[0]
    const files = await fetchResult.files()

    ino1 = statSync(getFilePathInCafs(cafsDir, files.filesIndex['package.json']['integrity'], 'nonexec')).ino

    expect(Object.keys(files.filesIndex).sort()).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort())
    expect(files.fromStore).toBeFalsy()

    expect(fetchResult.finishing).toBeTruthy()
  }

  {
    const fetchResult = fetchResults[1]
    const files = await fetchResult.files()

    ino2 = statSync(getFilePathInCafs(cafsDir, files.filesIndex['package.json']['integrity'], 'nonexec')).ino

    expect(Object.keys(files.filesIndex).sort()).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort())
    expect(files.fromStore).toBeFalsy()

    expect(fetchResult.finishing()).toBeTruthy()
  }

  expect(ino1).toBe(ino2)
})

test('fetchPackageToStore() does not cache errors', async () => {
  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .reply(404)

  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .replyWithFile(200, IS_POSTIVE_TARBALL)

  const noRetry = createClient({
    authConfig,
    retry: { retries: 0 },
    cacheDir: '.pnpm',
  })

  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)
  const packageRequester = createPackageRequester({
    resolve: noRetry.resolve,
    fetchers: noRetry.fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })

  const pkgId = `localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`

  const badRequest = packageRequester.fetchPackageToStore({
    force: false,
    lockfileDir: tempy.directory(),
    pkg: {
      name: 'is-positive',
      version: '1.0.0',
      id: pkgId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
  })
  await expect(badRequest.files()).rejects.toThrow()

  const fetchResult = packageRequester.fetchPackageToStore({
    force: false,
    lockfileDir: tempy.directory(),
    pkg: {
      name: 'is-positive',
      version: '1.0.0',
      id: pkgId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
  })
  const files = await fetchResult.files()
  expect(Object.keys(files.filesIndex).sort()).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort())
  expect(files.fromStore).toBeFalsy()

  expect(fetchResult.finishing()).toBeTruthy()
  expect(nock.isDone()).toBeTruthy()
})

// This test was added to cover the issue described here: https://github.com/pnpm/supi/issues/65
test('always return a package manifest in the response', async () => {
  nock.cleanAll()
  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })
  expect(typeof requestPackage).toBe('function')
  const projectDir = tempy.directory()

  {
    const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
      registry,
    }) as PackageResponse & { body: { manifest: { name: string } } }

    expect(pkgResponse.body).toBeTruthy()
    expect(pkgResponse.body.manifest.name).toBeTruthy()
  }

  {
    const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
      currentPkg: {
        id: `localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`,
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
        },
      },
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
      registry,
    }) as PackageResponse & { bundledManifest: () => Promise<DependencyManifest> }

    expect(pkgResponse.body).toBeTruthy()
    expect(
      await pkgResponse.bundledManifest()
    ).toStrictEqual(
      {
        engines: { node: '>=0.10.0' },
        name: 'is-positive',
        scripts: { test: 'node test.js' },
        version: '1.0.0',
      }
    )
  }
})

// Covers https://github.com/pnpm/pnpm/issues/1293
test('fetchPackageToStore() fetch raw manifest of cached package', async () => {
  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .replyWithFile(200, IS_POSTIVE_TARBALL)

  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)
  const packageRequester = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })

  const pkgId = `localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`
  const resolution = {
    registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
  }
  const fetchResults = await Promise.all([
    packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      lockfileDir: tempy.directory(),
      pkg: {
        name: 'is-positive',
        version: '1.0.0',
        id: pkgId,
        resolution,
      },
    }),
    packageRequester.fetchPackageToStore({
      fetchRawManifest: true,
      force: false,
      lockfileDir: tempy.directory(),
      pkg: {
        name: 'is-positive',
        version: '1.0.0',
        id: pkgId,
        resolution,
      },
    }),
  ])

  expect(await fetchResults[1].bundledManifest!()).toBeTruthy()
})

test('refetch package to store if it has been modified', async () => {
  nock.cleanAll()
  const storeDir = tempy.directory()
  const cafsDir = path.join(storeDir, 'files')
  const lockfileDir = tempy.directory()

  const pkgId = `localhost+${REGISTRY_MOCK_PORT}/magic-hook/2.0.0`
  const resolution = {
    registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/magic-hook/-/magic-hook-2.0.0.tgz`,
  }

  let indexJsFile!: string
  {
    const cafs = createCafsStore(storeDir)
    const packageRequester = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const fetchResult = packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      lockfileDir,
      pkg: {
        name: 'magic-hook',
        version: '2.0.0',
        id: pkgId,
        resolution,
      },
    })

    const { filesIndex } = await fetchResult.files()
    indexJsFile = getFilePathInCafs(cafsDir, filesIndex['index.js']['integrity'], 'nonexec')
  }

  await delay(200)
  // Adding some content to the file to change its integrity
  await fs.appendFile(indexJsFile, '// foobar')

  const reporter = jest.fn()
  streamParser.on('data', reporter)

  {
    const cafs = createCafsStore(storeDir)
    const packageRequester = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const fetchResult = packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      lockfileDir,
      pkg: {
        name: 'magic-hook',
        version: '2.0.0',
        id: pkgId,
        resolution,
      },
    })

    await fetchResult.files()
  }

  streamParser.removeListener('data', reporter)

  expect(await fs.readFile(indexJsFile, 'utf8')).not.toContain('// foobar')

  expect(reporter).toBeCalledWith(expect.objectContaining({
    level: 'warn',
    message: `Refetching ${path.join(storeDir, depPathToFilename(pkgId))} to store. It was either modified or had no integrity checksums`,
    name: 'pnpm:package-requester',
    prefix: lockfileDir,
  }))
})

test('do not fetch an optional package that is not installable', async () => {
  const storeDir = '.store'
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })
  expect(typeof requestPackage).toBe('function')

  const projectDir = tempy.directory()
  const pkgResponse = await requestPackage({ alias: '@pnpm.e2e/not-compatible-with-any-os', optional: true, pref: '*' }, {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
  })

  expect(pkgResponse).toBeTruthy()
  expect(pkgResponse.body).toBeTruthy()

  expect(pkgResponse.body.isInstallable).toBe(false)
  expect(pkgResponse.body.id).toBe(`localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/not-compatible-with-any-os/1.0.0`)

  expect(pkgResponse.files).toBeFalsy()
  expect(pkgResponse.finishing).toBeFalsy()
})

// Test case for https://github.com/pnpm/pnpm/issues/1866
test('fetch a git package without a package.json', async () => {
  // a small Deno library with a 'denolib.json' instead of a 'package.json'
  const repo = 'denolib/camelcase'
  const commit = 'aeb6b15f9c9957c8fa56f9731e914c4d8a6d2f2b'

  nock.cleanAll()
  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })
  expect(typeof requestPackage).toBe('function')
  const projectDir = tempy.directory()

  {
    const pkgResponse = await requestPackage({ alias: 'camelcase', pref: `${repo}#${commit}` }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
      registry,
    }) as PackageResponse & { body: { manifest: { name: string } } }

    expect(pkgResponse.body).toBeTruthy()
    expect(pkgResponse.body.manifest).toBeUndefined()
    expect(pkgResponse.body.isInstallable).toBeFalsy()
    expect(pkgResponse.body.id).toBe(`github.com/${repo}/${commit}`)
  }
})

test('throw exception if the package data in the store differs from the expected data', async () => {
  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)
  let pkgResponse!: PackageResponse

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const projectDir = tempy.directory()
    pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
      registry,
    })
    await pkgResponse.finishing!()
  }

  // Fail when the name of the package is different in the store
  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })
    const { files } = requestPackage.fetchPackageToStore({
      force: false,
      lockfileDir: tempy.directory(),
      pkg: {
        name: 'is-negative',
        version: '1.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
      expectedPkg: {
        name: 'is-negative',
        version: '1.0.0',
      },
    })
    await expect(files()).rejects.toThrow(/Package name mismatch found while reading/)
  }

  // Fail when the version of the package is different in the store
  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })
    const { files } = requestPackage.fetchPackageToStore({
      force: false,
      lockfileDir: tempy.directory(),
      pkg: {
        name: 'is-negative',
        version: '2.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
      expectedPkg: {
        name: 'is-negative',
        version: '2.0.0',
      },
    })
    await expect(files()).rejects.toThrow(/Package name mismatch found while reading/)
  }

  // Do not fail when the versions are the same but written in a differnt format (1.0.0 is the same as v1.0.0)
  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })
    const { files } = requestPackage.fetchPackageToStore({
      force: false,
      lockfileDir: tempy.directory(),
      pkg: {
        name: 'is-positive',
        version: 'v1.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
      expectedPkg: {
        name: 'is-positive',
        version: 'v1.0.0',
      },
    })
    await expect(files()).resolves.toStrictEqual(expect.anything())
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })
    const { files } = requestPackage.fetchPackageToStore({
      force: false,
      lockfileDir: tempy.directory(),
      pkg: {
        name: 'IS-positive',
        version: 'v1.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
      expectedPkg: {
        name: 'IS-positive',
        version: 'v1.0.0',
      },
    })
    await expect(files()).resolves.toStrictEqual(expect.anything())
  }
})

test("don't throw an error if the package was updated, so the expectedPkg has a different version than the version in the store", async () => {
  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)
  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const projectDir = tempy.directory()
    const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '3.1.0' }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
      registry,
    })
    await pkgResponse.finishing!()
  }
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })
  const projectDir = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '3.1.0' }, {
    downloadPriority: 0,
    lockfileDir: tempy.directory(),
    preferredVersions: {},
    projectDir,
    registry,
    expectedPkg: {
      name: 'is-positive',
      version: '3.0.0',
    },
  })
  await expect(pkgResponse.files!()).resolves.toStrictEqual(expect.anything())
})

test('the version in the bundled manifest should be normalized', async () => {
  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)

  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })

  const pkgResponse = await requestPackage({ alias: 'react-terminal', pref: '1.2.1' }, {
    downloadPriority: 0,
    lockfileDir: tempy.directory(),
    preferredVersions: {},
    projectDir: tempy.directory(),
    registry,
  })
  await expect(pkgResponse.bundledManifest!()).resolves.toStrictEqual(expect.objectContaining({
    version: '1.2.1',
  }))
  await pkgResponse.finishing!()
})

test('should skip store integrity check and resolve manifest if fetchRawManifest is true', async () => {
  const storeDir = tempy.directory()
  const cafs = createCafsStore(storeDir)

  let pkgResponse!: PackageResponse

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: false,
    })

    const projectDir = tempy.directory()

    pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
      registry,
    })

    await pkgResponse.finishing!()
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: false,
    })

    const fetchResult = requestPackage.fetchPackageToStore({
      force: false,
      fetchRawManifest: true,
      lockfileDir: tempy.directory(),
      pkg: {
        name: 'is-positive',
        version: '1.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
      expectedPkg: {
        name: 'is-positive',
        version: '1.0.0',
      },
    })

    await fetchResult.finishing()

    await expect(fetchResult.bundledManifest!()).resolves.toStrictEqual(expect.objectContaining({
      name: 'is-positive',
      version: '1.0.0',
    }))
  }
})
