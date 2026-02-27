/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'fs'
import path from 'path'
import { type PackageFilesIndex } from '@pnpm/store.cafs'
import { createClient } from '@pnpm/client'
import { readMsgpackFileSync } from '@pnpm/fs.msgpack-file'
import { streamParser } from '@pnpm/logger'
import { createPackageRequester, type PackageResponse } from '@pnpm/package-requester'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import delay from 'delay'
import { depPathToFilename } from '@pnpm/dependency-path'
import { restartWorkerPool } from '@pnpm/worker'
import { jest } from '@jest/globals'
import nock from 'nock'
import normalize from 'normalize-path'
import { temporaryDirectory } from 'tempy'
import { type PkgResolutionId, type PkgRequestFetchResult, type RequestPackageOptions } from '@pnpm/store-controller-types'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}`
const f = fixtures(import.meta.dirname)
const IS_POSITIVE_TARBALL = f.find('is-positive-1.0.0.tgz')

const registries = { default: registry }

const authConfig = { registry }

const { resolve, fetchers } = createClient({
  authConfig,
  cacheDir: '.store',
  storeDir: '.store',
  rawConfig: {},
  registries,
})

test('request package', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })
  expect(typeof requestPackage).toBe('function')

  const projectDir = temporaryDirectory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
  })

  expect(pkgResponse).toBeTruthy()
  expect(pkgResponse.body).toBeTruthy()

  expect(pkgResponse.body.id).toBe('is-positive@1.0.0')
  expect(pkgResponse.body.resolvedVia).toBe('npm-registry')
  expect(pkgResponse.body.isLocal).toBe(false)
  expect(typeof pkgResponse.body.latest).toBe('string')
  expect(pkgResponse.body.manifest?.name).toBe('is-positive')
  expect(!pkgResponse.body.normalizedBareSpecifier).toBeTruthy()
  expect(pkgResponse.body.resolution).toStrictEqual({
    integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
  })

  const { files } = await pkgResponse.fetching!()
  expect(Array.from(files.filesMap.keys()).sort((a, b) => a.localeCompare(b))).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort((a, b) => a.localeCompare(b)))
  expect(files.resolvedFrom).toBe('remote')
})

test('request package but skip fetching', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })
  expect(typeof requestPackage).toBe('function')

  const projectDir = temporaryDirectory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    skipFetch: true,
  })

  expect(pkgResponse).toBeTruthy()
  expect(pkgResponse.body).toBeTruthy()

  expect(pkgResponse.body.id).toBe('is-positive@1.0.0')
  expect(pkgResponse.body.isLocal).toBe(false)
  expect(typeof pkgResponse.body.latest).toBe('string')
  expect(pkgResponse.body.manifest?.name).toBe('is-positive')
  expect(!pkgResponse.body.normalizedBareSpecifier).toBeTruthy()
  expect(pkgResponse.body.resolution).toStrictEqual({
    integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
  })

  expect(pkgResponse.fetching).toBeFalsy()
})

test('request package but skip fetching, when resolution is already available', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })
  expect(typeof requestPackage).toBe('function')

  const projectDir = temporaryDirectory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
    currentPkg: {
      id: 'is-positive@1.0.0' as PkgResolutionId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    skipFetch: true,
    update: false,
  }) as PackageResponse & {
    body: {
      latest: string
      manifest: { name: string }
    }
  }

  expect(pkgResponse).toBeTruthy()
  expect(pkgResponse.body).toBeTruthy()

  expect(pkgResponse.body.id).toBe('is-positive@1.0.0')
  expect(pkgResponse.body.isLocal).toBe(false)
  expect(typeof pkgResponse.body.latest).toBe('string')
  expect(pkgResponse.body.manifest.name).toBe('is-positive')
  expect(!pkgResponse.body.normalizedBareSpecifier).toBeTruthy()
  expect(pkgResponse.body.resolution).toStrictEqual({
    integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
  })

  expect(pkgResponse.fetching).toBeFalsy()
})

test('refetch local tarball if its integrity has changed', async () => {
  const projectDir = temporaryDirectory()
  const tarballPath = path.join(projectDir, 'tarball.tgz')
  const tarballRelativePath = path.relative(projectDir, tarballPath)
  f.copy('pnpm-package-requester-0.8.1.tgz', tarballPath)
  const tarball = `file:${tarballRelativePath}`
  const wantedPackage = { bareSpecifier: tarball }
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const pkgId = `file:${normalize(tarballRelativePath)}`
  const requestPackageOpts = {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    skipFetch: true,
    update: false,
  } satisfies RequestPackageOptions

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      currentPkg: {
        id: pkgId as PkgResolutionId,
        resolution: {
          integrity: 'sha512-lqODmYcc/FKOGROEUByd5Sbugqhzgkv+Hij9PXH0sZVQsU2npTQ0x3L81GCtHilFKme8lhBtD31Vxg/AKYrAvg==',
          tarball,
        },
      },
    }) as PackageResponse & {
      fetching: () => Promise<PkgRequestFetchResult>
    }
    const { files, bundledManifest } = await response.fetching()

    expect(response.body.updated).toBeFalsy()
    expect(files.resolvedFrom).toBe('remote')
    expect(bundledManifest).toBeTruthy()
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
      virtualStoreDirMaxLength: 120,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      currentPkg: {
        id: pkgId as PkgResolutionId,
        resolution: {
          integrity: 'sha512-lqODmYcc/FKOGROEUByd5Sbugqhzgkv+Hij9PXH0sZVQsU2npTQ0x3L81GCtHilFKme8lhBtD31Vxg/AKYrAvg==',
          tarball,
        },
      },
    })
    const { files, bundledManifest } = await response.fetching!()

    expect(response.body.updated).toBeTruthy()
    expect(files.resolvedFrom).toBe('remote')
    expect(bundledManifest).toBeTruthy()
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      currentPkg: {
        id: pkgId as PkgResolutionId,
        resolution: {
          integrity: 'sha512-v3uhYkN+Eh3Nus4EZmegjQhrfpdPIH+2FjrkeBc6ueqZJWWRaLnSYIkD0An6m16D3v+6HCE18ox6t95eGxj5Pw==',
          tarball,
        },
      },
    }) as PackageResponse & {
      fetching: () => Promise<PkgRequestFetchResult>
    }
    const { files, bundledManifest } = await response.fetching()

    expect(response.body.updated).toBeFalsy()
    expect(files.resolvedFrom).toBe('store')
    expect(bundledManifest).toBeTruthy()
  }
})

test('refetch local tarball if its integrity has changed. The requester does not know the correct integrity', async () => {
  const projectDir = temporaryDirectory()
  const tarballPath = path.join(projectDir, 'tarball.tgz')
  f.copy('pnpm-package-requester-0.8.1.tgz', tarballPath)
  const tarball = `file:${tarballPath}`
  const wantedPackage = { bareSpecifier: tarball }
  const storeDir = path.join(projectDir, 'store')
  const cafs = createCafsStore(storeDir)
  const requestPackageOpts = {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    update: false,
  } satisfies RequestPackageOptions

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      fetching: () => Promise<PkgRequestFetchResult>
    }
    const { files, bundledManifest } = await response.fetching()

    expect(response.body.updated).toBeTruthy()
    expect(files.resolvedFrom).toBe('remote')
    expect(bundledManifest).toBeTruthy()
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
      virtualStoreDirMaxLength: 120,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      fetching: () => Promise<PkgRequestFetchResult>
    }
    const { files, bundledManifest } = await response.fetching()

    expect(response.body.updated).toBeTruthy()
    expect(files.resolvedFrom).toBe('remote')
    expect(bundledManifest).toBeTruthy()
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      storeDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      fetching: () => Promise<PkgRequestFetchResult>
    }
    const { files, bundledManifest } = await response.fetching()

    expect(files.resolvedFrom).toBe('store')
    expect(bundledManifest).toBeTruthy()
  }
})

test('force fetch when resolution integrity differs from current package integrity', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const projectDir = temporaryDirectory()

  // Create a custom resolver that returns a different integrity than the current package
  const customResolve: typeof resolve = async () => {
    // Return a resolution with a different integrity than what's in currentPkg
    return {
      id: 'is-positive@1.0.0' as PkgResolutionId,
      latest: '1.0.0',
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
      manifest: {
        name: 'is-positive',
        version: '1.0.0',
      },
      resolvedVia: 'npm-registry',
    }
  }

  const requestPackage = createPackageRequester({
    resolve: customResolve,
    fetchers,
    cafs,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })

  // Request with a currentPkg that has a different integrity
  const response = await requestPackage({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
    currentPkg: {
      id: 'is-positive@1.0.0' as PkgResolutionId,
      resolution: {
        // Different valid integrity than what the resolver returns
        integrity: 'sha512-AvAi2XyFuGzKkv+hij9PXH0sZVQsU2npTQ0x3L81GCtHilFKme8lhBtD31Vxg/AKYrAvg==',
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    skipFetch: false,
    update: false,
  }) as PackageResponse & {
    fetching: () => Promise<PkgRequestFetchResult>
  }

  // The package should be marked as updated because the integrity changed
  expect(response.body.updated).toBe(true)

  // Fetching should occur because integrity changed
  const { files } = await response.fetching()
  expect(files.resolvedFrom).toBe('remote')
})

test('fetchPackageToStore()', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const packageRequester = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })

  const pkgId = 'is-positive@1.0.0'
  const fetchResult = packageRequester.fetchPackageToStore({
    force: false,
    lockfileDir: temporaryDirectory(),
    pkg: {
      name: 'is-positive',
      version: '1.0.0',
      id: pkgId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
  })

  const { files, bundledManifest } = await fetchResult.fetching()
  expect(bundledManifest).toBeTruthy() // we always read the bundled manifest
  expect(Array.from(files.filesMap.keys()).sort((a, b) => a.localeCompare(b))).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort((a, b) => a.localeCompare(b)))
  expect(files.resolvedFrom).toBe('remote')

  const indexFile = readMsgpackFileSync<PackageFilesIndex>(fetchResult.filesIndexFile)
  expect(indexFile).toBeTruthy()
  expect(typeof indexFile.files.get('package.json')!.checkedAt).toBeTruthy()

  const fetchResult2 = packageRequester.fetchPackageToStore({
    fetchRawManifest: true,
    force: false,
    lockfileDir: temporaryDirectory(),
    pkg: {
      name: 'is-positive',
      version: '1.0.0',
      id: pkgId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
  })

  // This verifies that when a package has been cached with no full manifest
  // the full manifest is requested and added to the cache
  expect(
    (await fetchResult2.fetching()).bundledManifest
  ).toStrictEqual(
    {
      devDependencies: { ava: '^0.0.4' },
      engines: { node: '>=0.10.0' },
      name: 'is-positive',
      version: '1.0.0',
    }
  )
})

test('fetchPackageToStore() concurrency check', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const packageRequester = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })

  const pkgId = 'is-positive@1.0.0'
  const projectDir1 = temporaryDirectory()
  const projectDir2 = temporaryDirectory()
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
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
        },
      },
    }),
  ])

  let ino1!: number
  let ino2!: number

  {
    const fetchResult = fetchResults[0]
    const { files } = await fetchResult.fetching()

    ino1 = fs.statSync(files.filesMap.get('package.json') as string).ino

    expect(Array.from(files.filesMap.keys()).sort((a, b) => a.localeCompare(b))).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort((a, b) => a.localeCompare(b)))
    expect(files.resolvedFrom).toBe('remote')
  }

  {
    const fetchResult = fetchResults[1]
    const { files } = await fetchResult.fetching()

    ino2 = fs.statSync(files.filesMap.get('package.json') as string).ino

    expect(Array.from(files.filesMap.keys()).sort((a, b) => a.localeCompare(b))).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort((a, b) => a.localeCompare(b)))
    expect(files.resolvedFrom).toBe('remote')
  }

  expect(ino1).toBe(ino2)
})

test('fetchPackageToStore() does not cache errors', async () => {
  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .reply(404)

  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .replyWithFile(200, IS_POSITIVE_TARBALL)

  const noRetry = createClient({
    authConfig,
    rawConfig: {},
    retry: { retries: 0 },
    cacheDir: '.pnpm',
    storeDir: '.store',
    registries,
  })

  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const packageRequester = createPackageRequester({
    resolve: noRetry.resolve,
    fetchers: noRetry.fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })

  const pkgId = 'is-positive@1.0.0'

  const badRequest = packageRequester.fetchPackageToStore({
    force: false,
    lockfileDir: temporaryDirectory(),
    pkg: {
      name: 'is-positive',
      version: '1.0.0',
      id: pkgId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
  })
  await expect(badRequest.fetching()).rejects.toThrow()

  const fetchResult = packageRequester.fetchPackageToStore({
    force: false,
    lockfileDir: temporaryDirectory(),
    pkg: {
      name: 'is-positive',
      version: '1.0.0',
      id: pkgId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
      },
    },
  })
  const { files } = await fetchResult.fetching()
  expect(Array.from(files.filesMap.keys()).sort((a, b) => a.localeCompare(b))).toStrictEqual(['package.json', 'index.js', 'license', 'readme.md'].sort((a, b) => a.localeCompare(b)))
  expect(files.resolvedFrom).toBe('remote')

  expect(nock.isDone()).toBeTruthy()
})

// This test was added to cover the issue described here: https://github.com/pnpm/supi/issues/65
test('always return a package manifest in the response', async () => {
  nock.cleanAll()
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })
  expect(typeof requestPackage).toBe('function')
  const projectDir = temporaryDirectory()

  {
    const pkgResponse = await requestPackage({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    }) as PackageResponse & { body: { manifest: { name: string } } }

    expect(pkgResponse.body).toBeTruthy()
    expect(pkgResponse.body.manifest.name).toBeTruthy()
  }

  {
    const pkgResponse = await requestPackage({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
      currentPkg: {
        id: 'is-positive@1.0.0' as PkgResolutionId,
        resolution: {
          integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
        },
      },
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    }) as PackageResponse & { fetching: () => Promise<PkgRequestFetchResult> }

    expect(pkgResponse.body).toBeTruthy()
    expect(
      (await pkgResponse.fetching()).bundledManifest
    ).toEqual(
      {
        devDependencies: { ava: '^0.0.4' },
        engines: { node: '>=0.10.0' },
        name: 'is-positive',
        version: '1.0.0',
      }
    )
  }
})

// Covers https://github.com/pnpm/pnpm/issues/1293
test('fetchPackageToStore() fetch raw manifest of cached package', async () => {
  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .replyWithFile(200, IS_POSITIVE_TARBALL)

  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const packageRequester = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })

  const pkgId = 'is-positive@1.0.0'
  const resolution = {
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
  }
  const fetchResults = await Promise.all([
    packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      lockfileDir: temporaryDirectory(),
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
      lockfileDir: temporaryDirectory(),
      pkg: {
        name: 'is-positive',
        version: '1.0.0',
        id: pkgId,
        resolution,
      },
    }),
  ])

  expect((await fetchResults[1].fetching()).bundledManifest).toBeTruthy()
})

test('refetch package to store if it has been modified', async () => {
  nock.cleanAll()
  const storeDir = temporaryDirectory()
  const lockfileDir = temporaryDirectory()

  const pkgId = 'magic-hook@2.0.0'
  const resolution = {
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
      virtualStoreDirMaxLength: 120,
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

    const { filesMap } = (await fetchResult.fetching()).files
    indexJsFile = filesMap.get('index.js') as string
  }

  // We should restart the workers otherwise the locker cache will still try to read the file
  // that will be removed from the store due to integrity change
  await restartWorkerPool()

  await delay(200)
  // Adding some content to the file to change its integrity
  fs.appendFileSync(indexJsFile, '// foobar')

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
      virtualStoreDirMaxLength: 120,
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

    await fetchResult.fetching()
  }

  streamParser.removeListener('data', reporter)

  expect(fs.readFileSync(indexJsFile, 'utf8')).not.toContain('// foobar')

  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    level: 'warn',
    message: `Refetching ${path.join(storeDir, depPathToFilename(pkgId, 120))} to store. It was either modified or had no integrity checksums`,
    name: 'pnpm:package-requester',
    prefix: lockfileDir,
  }))
})

test('do not fetch an optional package that is not installable', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })
  expect(typeof requestPackage).toBe('function')

  const projectDir = temporaryDirectory()
  const pkgResponse = await requestPackage({ alias: '@pnpm.e2e/not-compatible-with-any-os', optional: true, bareSpecifier: '*' }, {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
  })

  expect(pkgResponse).toBeTruthy()
  expect(pkgResponse.body).toBeTruthy()

  expect(pkgResponse.body.isInstallable).toBe(false)
  expect(pkgResponse.body.id).toBe('@pnpm.e2e/not-compatible-with-any-os@1.0.0')

  expect(pkgResponse.fetching).toBeFalsy()
})

// Test case for https://github.com/pnpm/pnpm/issues/1866
test('fetch a git package without a package.json', async () => {
  // a small Deno library with a 'denolib.json' instead of a 'package.json'
  const repo = 'denolib/camelcase'
  const commit = 'aeb6b15f9c9957c8fa56f9731e914c4d8a6d2f2b'

  nock.cleanAll()
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })
  expect(typeof requestPackage).toBe('function')
  const projectDir = temporaryDirectory()

  {
    const pkgResponse = await requestPackage({ alias: 'camelcase', bareSpecifier: `${repo}#${commit}` }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    }) as PackageResponse & { body: { manifest: { name: string } } }

    expect(pkgResponse.body).toBeTruthy()
    expect(pkgResponse.body.manifest).toBeUndefined()
    expect(pkgResponse.body.isInstallable).toBeFalsy()
    expect(pkgResponse.body.id).toBe(`https://codeload.github.com/${repo}/tar.gz/${commit}`)
  }
})

test('throw exception if the package data in the store differs from the expected data', async () => {
  const storeDir = temporaryDirectory()
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
      virtualStoreDirMaxLength: 120,
    })

    const projectDir = temporaryDirectory()
    pkgResponse = await requestPackage({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    })
    await pkgResponse.fetching!()
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
      virtualStoreDirMaxLength: 120,
    })
    const { fetching } = requestPackage.fetchPackageToStore({
      force: false,
      lockfileDir: temporaryDirectory(),
      pkg: {
        name: 'is-negative',
        version: '1.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
    })
    await expect(fetching()).rejects.toThrow(/Package name or version mismatch found while reading/)
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
      virtualStoreDirMaxLength: 120,
    })
    const { fetching } = requestPackage.fetchPackageToStore({
      force: false,
      lockfileDir: temporaryDirectory(),
      pkg: {
        name: 'is-negative',
        version: '2.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
    })
    await expect(fetching()).rejects.toThrow(/Package name or version mismatch found while reading/)
  }

  // Do not fail when the versions are the same but written in a different format (1.0.0 is the same as v1.0.0)
  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
    })
    const { fetching } = requestPackage.fetchPackageToStore({
      force: false,
      lockfileDir: temporaryDirectory(),
      pkg: {
        name: 'is-positive',
        version: 'v1.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
    })
    await expect(fetching()).resolves.toStrictEqual(expect.anything())
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
    })
    const { fetching } = requestPackage.fetchPackageToStore({
      force: false,
      lockfileDir: temporaryDirectory(),
      pkg: {
        name: 'IS-positive',
        version: 'v1.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
    })
    await expect(fetching()).resolves.toStrictEqual(expect.anything())
  }
})

test("don't throw an error if the package was updated, so the expectedPkg has a different version than the version in the store", async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
      virtualStoreDirMaxLength: 120,
    })

    const projectDir = temporaryDirectory()
    const pkgResponse = await requestPackage({ alias: 'is-positive', bareSpecifier: '3.1.0' }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    })
    await pkgResponse.fetching!()
  }
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })
  const projectDir = temporaryDirectory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', bareSpecifier: '3.1.0' }, {
    downloadPriority: 0,
    lockfileDir: temporaryDirectory(),
    preferredVersions: {},
    projectDir,
    expectedPkg: {
      name: 'is-positive',
      version: '3.0.0',
    },
  })
  await expect(pkgResponse.fetching!()).resolves.toStrictEqual(expect.anything())
})

test('the version in the bundled manifest should be normalized', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)

  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })

  const pkgResponse = await requestPackage({ alias: 'react-terminal', bareSpecifier: '1.2.1' }, {
    downloadPriority: 0,
    lockfileDir: temporaryDirectory(),
    preferredVersions: {},
    projectDir: temporaryDirectory(),
  })
  expect((await pkgResponse.fetching!()).bundledManifest?.version).toBe('1.2.1')
})

test('should skip store integrity check and resolve manifest if fetchRawManifest is true', async () => {
  const storeDir = temporaryDirectory()
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
      virtualStoreDirMaxLength: 120,
    })

    const projectDir = temporaryDirectory()

    pkgResponse = await requestPackage({ alias: 'is-positive', bareSpecifier: '1.0.0' }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    })

    await pkgResponse.fetching!()
  }

  {
    const requestPackage = createPackageRequester({
      resolve,
      fetchers,
      cafs,
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: false,
      virtualStoreDirMaxLength: 120,
    })

    const fetchResult = requestPackage.fetchPackageToStore({
      force: false,
      fetchRawManifest: true,
      lockfileDir: temporaryDirectory(),
      pkg: {
        name: 'is-positive',
        version: '1.0.0',
        id: pkgResponse.body.id,
        resolution: pkgResponse.body.resolution,
      },
    })

    await fetchResult.fetching()

    expect((await fetchResult.fetching!()).bundledManifest).toMatchObject({
      name: 'is-positive',
      version: '1.0.0',
      devDependencies: {
        ava: '^0.0.4',
      },
    })
  }
})

test('HTTP tarball without integrity gets integrity computed during fetch', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)
  const requestPackage = createPackageRequester({
    resolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })

  const projectDir = temporaryDirectory()
  // Request a package via HTTP tarball URL (simulated via the local registry)
  const pkgResponse = await requestPackage(
    { alias: 'is-positive', bareSpecifier: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz` },
    {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    }
  )

  expect(pkgResponse.body).toBeTruthy()
  // The resolution should now include an integrity hash computed during fetch
  expect(pkgResponse.body.resolution).toHaveProperty('integrity')
  expect((pkgResponse.body.resolution as { integrity?: string }).integrity).toMatch(/^sha512-/)
})

test('should pass optional flag to resolve function', async () => {
  const storeDir = temporaryDirectory()
  const cafs = createCafsStore(storeDir)

  let capturedOptional: boolean | undefined
  const mockResolve: typeof resolve = async (wantedDependency, _options) => {
    capturedOptional = wantedDependency.optional
    return resolve(wantedDependency, _options)
  }

  const requestPackage = createPackageRequester({
    resolve: mockResolve,
    fetchers,
    cafs,
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
    virtualStoreDirMaxLength: 120,
  })

  const projectDir = temporaryDirectory()

  await requestPackage(
    { alias: 'is-positive', bareSpecifier: '1.0.0', optional: true },
    {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    }
  )

  expect(capturedOptional).toBe(true)

  await requestPackage(
    { alias: 'is-positive', bareSpecifier: '1.0.0', optional: false },
    {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    }
  )

  expect(capturedOptional).toBe(false)

  await requestPackage(
    { alias: 'is-positive', bareSpecifier: '1.0.0' },
    {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
    }
  )

  expect(capturedOptional).toBeUndefined()
})
