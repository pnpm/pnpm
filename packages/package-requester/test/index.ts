///<reference path="../typings/index.d.ts" />
import localResolver from '@pnpm/local-resolver'
import { streamParser } from '@pnpm/logger'
import createResolver from '@pnpm/npm-resolver'
import createPackageRequester, { PackageFilesResponse, PackageResponse } from '@pnpm/package-requester'
import { ResolveFunction } from '@pnpm/resolver-base'
import createFetcher from '@pnpm/tarball-fetcher'
import { PackageJson } from '@pnpm/types'
import delay from 'delay'
import loadJsonFile from 'load-json-file'
import fs = require('mz/fs')
import ncpCB = require('ncp')
import nock = require('nock')
import normalize = require('normalize-path')
import path = require('path')
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import test = require('tape')
import tempy = require('tempy')
import promisify = require('util.promisify')

const registry = 'https://registry.npmjs.org/'
const IS_POSTIVE_TARBALL = path.join(__dirname, 'is-positive-1.0.0.tgz')
const ncp = promisify(ncpCB)

const rawNpmConfig = { registry }

const resolve = createResolver({
  metaCache: new Map(),
  rawNpmConfig,
  store: '.store',
})
const fetch = createFetcher({
  alwaysAuth: false,
  rawNpmConfig,
  registry: 'https://registry.npmjs.org/',
  strictSsl: false,
})

test('request package', async t => {
  const storeIndex = {}
  const requestPackage = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storeIndex,
    storePath: '.store',
  })
  t.equal(typeof requestPackage, 'function')

  const prefix = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
    downloadPriority: 0,
    lockfileDirectory: prefix,
    loggedPkg: {
      rawSpec: 'is-positive@1.0.0',
    },
    preferredVersions: {},
    prefix,
    registry,
    verifyStoreIntegrity: true,
  }) as PackageResponse & {
    body: {inStoreLocation: string, latest: string, manifest: {name: string}},
    fetchingFiles: Promise<{filenames: string[], fromStore: boolean}>,
    finishing: Promise<void>,
  }

  t.ok(pkgResponse, 'response received')
  t.ok(pkgResponse.body, 'response has body')

  t.equal(pkgResponse.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct package ID')
  t.equal(pkgResponse.body.resolvedVia, 'npm-registry', 'responded with correct resolvedVia')
  t.equal(pkgResponse.body.inStoreLocation, path.join('.store', 'registry.npmjs.org', 'is-positive', '1.0.0'), 'package location in store returned')
  t.equal(pkgResponse.body.isLocal, false, 'package is not local')
  t.equal(typeof pkgResponse.body.latest, 'string', 'latest is returned')
  t.equal(pkgResponse.body.manifest.name, 'is-positive', 'package manifest returned')
  t.ok(!pkgResponse.body.normalizedPref, 'no normalizedPref returned')
  t.deepEqual(pkgResponse.body.resolution, {
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  }, 'resolution returned')

  const files = await pkgResponse.fetchingFiles
  t.deepEqual(files, {
    filenames: [ 'package.json', 'index.js', 'license', 'readme.md' ],
    fromStore: false,
  }, 'returned info about files after fetch completed')

  t.ok(pkgResponse.finishing)

  t.deepEqual(storeIndex, { 'registry.npmjs.org/is-positive/1.0.0': [] })

  t.end()
})

test('request package but skip fetching', async t => {
  const requestPackage = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storeIndex: {},
    storePath: '.store',
  })
  t.equal(typeof requestPackage, 'function')

  const prefix = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
    downloadPriority: 0,
    lockfileDirectory: prefix,
    loggedPkg: {
      rawSpec: 'is-positive@1.0.0',
    },
    preferredVersions: {},
    prefix,
    registry,
    skipFetch: true,
    verifyStoreIntegrity: true,
  }) as PackageResponse & {
    body: {inStoreLocation: string, latest: string, manifest: {name: string}},
    fetchingFiles: Promise<object>,
    finishing: Promise<void>,
  }

  t.ok(pkgResponse, 'response received')
  t.ok(pkgResponse.body, 'response has body')

  t.equal(pkgResponse.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct package ID')
  t.equal(pkgResponse.body.inStoreLocation, path.join('.store', 'registry.npmjs.org', 'is-positive', '1.0.0'), 'package location in store returned')
  t.equal(pkgResponse.body.isLocal, false, 'package is not local')
  t.equal(typeof pkgResponse.body.latest, 'string', 'latest is returned')
  t.equal(pkgResponse.body.manifest.name, 'is-positive', 'package manifest returned')
  t.ok(!pkgResponse.body.normalizedPref, 'no normalizedPref returned')
  t.deepEqual(pkgResponse.body.resolution, {
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  }, 'resolution returned')

  t.notOk(pkgResponse.fetchingFiles, 'files fetching not done')
  t.notOk(pkgResponse.finishing)

  t.end()
})

test('request package but skip fetching, when resolution is already available', async t => {
  const requestPackage = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storeIndex: {},
    storePath: '.store',
  })
  t.equal(typeof requestPackage, 'function')

  const prefix = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
    currentPkgId: 'registry.npmjs.org/is-positive/1.0.0',
    downloadPriority: 0,
    lockfileDirectory: prefix,
    loggedPkg: {
      rawSpec: 'is-positive@1.0.0',
    },
    preferredVersions: {},
    prefix,
    registry,
    shrinkwrapResolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
    skipFetch: true,
    update: false,
    verifyStoreIntegrity: true,
  }) as PackageResponse & {
    body: {
      inStoreLocation: string,
      latest: string,
      manifest: {name: string},
    },
    fetchingFiles: Promise<object>,
    finishing: Promise<void>,
  }

  t.ok(pkgResponse, 'response received')
  t.ok(pkgResponse.body, 'response has body')

  t.equal(pkgResponse.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct package ID')
  t.equal(pkgResponse.body.inStoreLocation, path.join('.store', 'registry.npmjs.org', 'is-positive', '1.0.0'), 'package location in store returned')
  t.equal(pkgResponse.body.isLocal, false, 'package is not local')
  t.equal(typeof pkgResponse.body.latest, 'string', 'latest is returned')
  t.equal(pkgResponse.body.manifest.name, 'is-positive', 'package manifest returned')
  t.ok(!pkgResponse.body.normalizedPref, 'no normalizedPref returned')
  t.deepEqual(pkgResponse.body.resolution, {
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  }, 'resolution returned')

  t.notOk(pkgResponse.fetchingFiles, 'files fetching not done')
  t.notOk(pkgResponse.finishing)

  t.end()
})

test('refetch local tarball if its integrity has changed', async t => {
  const prefix = tempy.directory()
  const tarballPath = path.join(prefix, 'tarball.tgz')
  const tarballRelativePath = path.relative(prefix, tarballPath)
  await ncp(path.join(__dirname, 'pnpm-package-requester-0.8.1.tgz'), tarballPath)
  const tarball = `file:${tarballRelativePath}`
  const wantedPackage = { pref: tarball }
  const storePath = path.join(__dirname, '..', '.store')
  const pkgId = `file:${normalize(tarballRelativePath)}`
  const requestPackageOpts = {
    currentPkgId: pkgId,
    downloadPriority: 0,
    lockfileDirectory: prefix,
    loggedPkg: {
      rawSpec: tarball,
    },
    preferredVersions: {},
    prefix,
    registry,
    skipFetch: true,
    update: false,
    verifyStoreIntegrity: true,
  }
  const storeIndex = {}

  {
    const requestPackage = createPackageRequester(localResolver as ResolveFunction, fetch, {
      storeIndex,
      storePath,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      shrinkwrapResolution: {
        integrity: 'sha512-lqODmYcc/FKOGROEUByd5Sbugqhzgkv+Hij9PXH0sZVQsU2npTQ0x3L81GCtHilFKme8lhBtD31Vxg/AKYrAvg==',
        tarball,
      },
    }) as PackageResponse & {
      fetchingFiles: Promise<PackageFilesResponse>,
      finishing: Promise<void>,
    }
    await response.fetchingFiles
    await response.finishing

    t.ok(response.body.updated === false, 'resolution not updated')
    t.notOk((await response.fetchingFiles).fromStore, 'unpack tarball if it is not in store yet')
    t.equal((await response['fetchingRawManifest']).version, '0.8.1')
  }

  await ncp(path.join(__dirname, 'pnpm-package-requester-4.1.2.tgz'), tarballPath)
  await delay(50)

  {
    const requestPackage = createPackageRequester(localResolver as ResolveFunction, fetch, {
      storeIndex,
      storePath,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      shrinkwrapResolution: {
        integrity: 'sha512-lqODmYcc/FKOGROEUByd5Sbugqhzgkv+Hij9PXH0sZVQsU2npTQ0x3L81GCtHilFKme8lhBtD31Vxg/AKYrAvg==',
        tarball,
      },
    }) as PackageResponse & {
      fetchingFiles: Promise<PackageFilesResponse>,
      finishing: Promise<void>,
    }
    await response.fetchingFiles
    await response.finishing

    t.ok(response.body.updated === true, 'resolution updated')
    t.notOk((await response.fetchingFiles).fromStore, 'reunpack tarball if its integrity is not up-to-date')
    t.equal((await response['fetchingRawManifest']).version, '4.1.2')
  }

  {
    const requestPackage = createPackageRequester(localResolver as ResolveFunction, fetch, {
      storeIndex,
      storePath,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      shrinkwrapResolution: {
        integrity: 'sha512-v3uhYkN+Eh3Nus4EZmegjQhrfpdPIH+2FjrkeBc6ueqZJWWRaLnSYIkD0An6m16D3v+6HCE18ox6t95eGxj5Pw==',
        tarball,
      },
    }) as PackageResponse & {
      fetchingFiles: Promise<PackageFilesResponse>,
      finishing: Promise<void>,
    }
    await response.fetchingFiles
    await response.finishing

    t.ok(response.body.updated === false, 'resolution not updated')
    t.ok((await response.fetchingFiles).fromStore, 'do not reunpack tarball if its integrity is up-to-date')
    t.equal((await response['fetchingRawManifest']).version, '4.1.2')
  }

  t.end()
})

test('refetch local tarball if its integrity has changed. The requester does not know the correct integrity', async t => {
  const prefix = tempy.directory()
  const tarballPath = path.join(prefix, 'tarball.tgz')
  await ncp(path.join(__dirname, 'pnpm-package-requester-0.8.1.tgz'), tarballPath)
  const tarball = `file:${tarballPath}`
  const wantedPackage = { pref: tarball }
  const storePath = path.join(__dirname, '..', '.store')
  const requestPackageOpts = {
    downloadPriority: 0,
    lockfileDirectory: prefix,
    loggedPkg: {
      rawSpec: tarball,
    },
    preferredVersions: {},
    prefix,
    registry,
    update: false,
    verifyStoreIntegrity: true,
  }
  const storeIndex = {}

  {
    const requestPackage = createPackageRequester(localResolver as ResolveFunction, fetch, {
      storeIndex,
      storePath,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      fetchingFiles: Promise<PackageFilesResponse>,
      finishing: Promise<void>,
    }
    await response.fetchingFiles
    await response.finishing

    t.ok(response.body.updated === true, 'resolution updated')
    t.notOk((await response.fetchingFiles).fromStore, 'unpack tarball if it is not in store yet')
    t.equal((await response['fetchingRawManifest']).version, '0.8.1')
  }

  await ncp(path.join(__dirname, 'pnpm-package-requester-4.1.2.tgz'), tarballPath)
  await delay(50)

  {
    const requestPackage = createPackageRequester(localResolver as ResolveFunction, fetch, {
      storeIndex,
      storePath,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      fetchingFiles: Promise<PackageFilesResponse>,
      finishing: Promise<void>,
    }
    await response.fetchingFiles
    await response.finishing

    t.ok(response.body.updated === true, 'resolution updated')
    t.notOk((await response.fetchingFiles).fromStore, 'reunpack tarball if its integrity is not up-to-date')
    t.equal((await response['fetchingRawManifest']).version, '4.1.2')
  }

  {
    const requestPackage = createPackageRequester(localResolver as ResolveFunction, fetch, {
      storeIndex,
      storePath,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      fetchingFiles: Promise<PackageFilesResponse>,
      finishing: Promise<void>,
    }
    await response.fetchingFiles
    await response.finishing

    t.ok((await response.fetchingFiles).fromStore, 'do not reunpack tarball if its integrity is up-to-date')
    t.equal((await response['fetchingRawManifest']).version, '4.1.2')
  }

  t.end()
})

test('fetchPackageToStore()', async (t) => {
  const packageRequester = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storeIndex: {},
    storePath: '.store',
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const storePath = '.store'
  const fetchResult = await packageRequester.fetchPackageToStore({
    force: false,
    pkgId,
    prefix: tempy.directory(),
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
    verifyStoreIntegrity: true,
  })

  t.notOk(fetchResult.fetchingRawManifest, 'full manifest not returned')

  const files = await fetchResult.fetchingFiles
  t.deepEqual(files, {
    filenames: [ 'package.json', 'index.js', 'license', 'readme.md' ],
    fromStore: false,
  }, 'returned info about files after fetch completed')

  t.ok(fetchResult.finishing)

  const fetchResult2 = await packageRequester.fetchPackageToStore({
    fetchRawManifest: true,
    force: false,
    pkgId,
    prefix: tempy.directory(),
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
    verifyStoreIntegrity: true,
  })

  // This verifies that when a package has been cached with no full manifest
  // the full manifest is requested and added to the cache
  t.ok((await fetchResult2.fetchingRawManifest)!.name, 'full manifest returned')

  t.end()
})

test('fetchPackageToStore() concurrency check', async (t) => {
  const packageRequester = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storeIndex: {},
    storePath: '.store',
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const prefix1 = tempy.directory()
  const prefix2 = tempy.directory()
  const fetchResults = await Promise.all([
    packageRequester.fetchPackageToStore({
      force: false,
      pkgId,
      prefix: prefix1,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
      verifyStoreIntegrity: true,
    }),
    packageRequester.fetchPackageToStore({
      force: false,
      pkgId,
      prefix: prefix2,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
      verifyStoreIntegrity: true,
    })
  ])

  let ino1!: Number
  let ino2!: Number

  {
    const fetchResult = await fetchResults[0]
    const files = await fetchResult.fetchingFiles

    ino1 = fs.statSync(path.join(fetchResult.inStoreLocation, 'package', 'package.json')).ino

    t.deepEqual(files, {
      filenames: [ 'package.json', 'index.js', 'license', 'readme.md' ],
      fromStore: false,
    }, 'returned info about files after fetch completed')

    t.ok(fetchResult.finishing)
  }

  {
    const fetchResult = await fetchResults[1]
    const files = await fetchResult.fetchingFiles

    ino2 = fs.statSync(path.join(fetchResult.inStoreLocation, 'package', 'package.json')).ino

    t.deepEqual(files, {
      filenames: [ 'package.json', 'index.js', 'license', 'readme.md' ],
      fromStore: false,
    }, 'returned info about files after fetch completed')

    t.ok(fetchResult.finishing)
  }

  t.equal(ino1, ino2, 'package fetched only once to the store')

  t.end()
})

test('fetchPackageToStore() does not cache errors', async (t) => {
  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .reply(404)

  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .replyWithFile(200, IS_POSTIVE_TARBALL)

  const noRetryFetch = createFetcher({
    alwaysAuth: false,
    fetchRetries: 0,
    rawNpmConfig,
    registry: 'https://registry.npmjs.org/',
    strictSsl: false,
  })

  const packageRequester = createPackageRequester(resolve, noRetryFetch, {
    networkConcurrency: 1,
    storeIndex: {},
    storePath: tempy.directory(),
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'

  try {
    const badRequest = await packageRequester.fetchPackageToStore({
      force: false,
      pkgId,
      prefix: tempy.directory(),
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
      verifyStoreIntegrity: true,
    })
    await badRequest.fetchingFiles
    t.fail('first fetch should have failed')
  } catch (err) {
    t.pass('first fetch failed')
  }

  const fetchResult = await packageRequester.fetchPackageToStore({
    force: false,
    pkgId,
    prefix: tempy.directory(),
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
    verifyStoreIntegrity: true,
  })
  const files = await fetchResult.fetchingFiles
  t.deepEqual(files, {
    filenames: [ 'package.json', 'index.js', 'license', 'readme.md' ],
    fromStore: false,
  }, 'returned info about files after fetch completed')

  t.ok(fetchResult.finishing)
  t.ok(nock.isDone())

  t.end()
})

// This test was added to cover the issue described here: https://github.com/pnpm/supi/issues/65
test('always return a package manifest in the response', async t => {
  const requestPackage = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storeIndex: {},
    storePath: '.store',
  })
  t.equal(typeof requestPackage, 'function')
  const prefix = tempy.directory()

  {
    const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
      downloadPriority: 0,
      lockfileDirectory: prefix,
      loggedPkg: {
        rawSpec: 'is-positive@1.0.0',
      },
      preferredVersions: {},
      prefix,
      registry,
      verifyStoreIntegrity: true,
    }) as PackageResponse & {body: {manifest: {name: string}}}

    t.ok(pkgResponse.body, 'response has body')
    t.ok(pkgResponse.body.manifest.name, 'response has manifest')
  }

  {
    const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
      currentPkgId: 'registry.npmjs.org/is-positive/1.0.0',
      downloadPriority: 0,
      lockfileDirectory: prefix,
      loggedPkg: {
        rawSpec: 'is-positive@1.0.0',
      },
      preferredVersions: {},
      prefix,
      registry,
      shrinkwrapResolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
      verifyStoreIntegrity: true,
    }) as PackageResponse & {fetchingRawManifest: Promise<PackageJson>}

    t.ok(pkgResponse.body, 'response has body')
    t.ok((await pkgResponse.fetchingRawManifest).name, 'response has manifest')
  }

  t.end()
})

// Covers https://github.com/pnpm/pnpm/issues/1293
test('fetchPackageToStore() fetch raw manifest of cached package', async (t) => {
  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .replyWithFile(200, IS_POSTIVE_TARBALL)

  const packageRequester = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storeIndex: {},
    storePath: tempy.directory(),
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const resolution = {
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  }
  const fetchResults = await Promise.all([
    packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      pkgId,
      prefix: tempy.directory(),
      resolution,
      verifyStoreIntegrity: true,
    }),
    packageRequester.fetchPackageToStore({
      fetchRawManifest: true,
      force: false,
      pkgId,
      prefix: tempy.directory(),
      resolution,
      verifyStoreIntegrity: true,
    })
  ])

  t.ok(await fetchResults[1].fetchingRawManifest)
  t.end()
})

test('refetch package to store if it has been modified', async (t) => {
  nock.cleanAll()
  const storePath = tempy.directory()
  const storeIndex = {}
  t.comment(`store location: ${storePath}`)

  const pkgId = 'registry.npmjs.org/magic-hook/2.0.0'
  const resolution = {
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/magic-hook/-/magic-hook-2.0.0.tgz',
  }

  {
    const packageRequester = createPackageRequester(resolve, fetch, {
      networkConcurrency: 1,
      storeIndex,
      storePath,
    })

    const fetchResult = await packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      pkgId,
      prefix: tempy.directory(),
      resolution,
      verifyStoreIntegrity: true,
    })

    await fetchResult.fetchingFiles
  }

  const distPathInStore = await path.join(storePath, pkgId, 'node_modules', 'magic-hook', 'dist')

  t.ok(await fs.exists(distPathInStore), `${distPathInStore} exists`)

  await rimraf(distPathInStore)

  t.notOk(await fs.exists(distPathInStore), `${distPathInStore} not exists`)

  const reporter = sinon.spy()
  streamParser.on('data', reporter)

  {
    const packageRequester = createPackageRequester(resolve, fetch, {
      networkConcurrency: 1,
      storeIndex,
      storePath,
    })

    const fetchResult = await packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      pkgId,
      prefix: tempy.directory(),
      resolution,
      verifyStoreIntegrity: true,
    })

    await fetchResult.fetchingFiles
  }

  streamParser.removeListener('data', reporter)

  t.ok(await fs.exists(distPathInStore), `${distPathInStore} exists`)

  t.ok(reporter.calledWithMatch({
    level: 'warn',
    message: `Refetching ${path.join(storePath, pkgId)} to store. It was either modified or had no integrity checksums`,
    name: 'pnpm:store',
  }), 'refetch logged')

  t.end()
})

test('refetch package to store if it has no integrity checksums and verification is needed', async (t) => {
  nock.cleanAll()
  const storePath = tempy.directory()
  const storeIndex = {}
  t.comment(`store location: ${storePath}`)

  const pkgId = 'registry.npmjs.org/magic-hook/2.0.0'
  const resolution = {
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/magic-hook/-/magic-hook-2.0.0.tgz',
  }

  {
    const packageRequester = createPackageRequester(resolve, fetch, {
      networkConcurrency: 1,
      storeIndex,
      storePath,
    })

    const fetchResult = await packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      pkgId,
      prefix: tempy.directory(),
      resolution,
      verifyStoreIntegrity: false,
    })

    await fetchResult.fetchingFiles

    const integrityJson = await loadJsonFile<object>(path.join(storePath, pkgId, 'integrity.json'))
    t.notOk(integrityJson['package.json'].integrity, 'no integrity hash generated')
  }

  const reporter = sinon.spy()
  streamParser.on('data', reporter)

  {
    const packageRequester = createPackageRequester(resolve, fetch, {
      networkConcurrency: 1,
      storeIndex,
      storePath,
    })

    const fetchResult = await packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      pkgId,
      prefix: tempy.directory(),
      resolution,
      verifyStoreIntegrity: true,
    })

    await fetchResult.fetchingFiles

    const integrityJson = await loadJsonFile<object>(path.join(storePath, pkgId, 'integrity.json'))
    t.ok(integrityJson['package.json'].integrity, 'integrity hash generated')
  }

  streamParser.removeListener('data', reporter)

  t.ok(reporter.calledWithMatch({
    level: 'warn',
    message: `Refetching ${path.join(storePath, pkgId)} to store. It was either modified or had no integrity checksums`,
    name: 'pnpm:store',
  }), 'refetch logged')

  t.end()
})
