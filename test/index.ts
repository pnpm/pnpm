import test = require('tape')
import {FetchFunction} from '@pnpm/fetcher-base'
import createPackageRequester, { PackageResponse, PackageFilesResponse } from '@pnpm/package-requester'
import createResolver from '@pnpm/npm-resolver'
import createFetcher from '@pnpm/tarball-fetcher'
import {PackageJson} from '@pnpm/types'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import fs = require('fs')
import path = require('path')
import tempy = require('tempy')
import nock = require('nock')

const registry = 'https://registry.npmjs.org/'

const rawNpmConfig = { registry }

const resolve = createResolver({
  rawNpmConfig,
  metaCache: new Map(),
  store: '.store',
})
const fetch = createFetcher({
  alwaysAuth: false,
  registry: 'https://registry.npmjs.org/',
  strictSsl: false,
  rawNpmConfig,
})

test('request package', async t => {
  const storeIndex = {}
  const requestPackage = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storePath: '.store',
    storeIndex,
  })
  t.equal(typeof requestPackage, 'function')

  const pkgResponse = await requestPackage({alias: 'is-positive', pref: '1.0.0'}, {
    downloadPriority: 0,
    loggedPkg: {
      rawSpec: 'is-positive@1.0.0',
    },
    prefix: tempy.directory(),
    registry,
    verifyStoreIntegrity: true,
    preferredVersions: {},
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
    storePath: '.store',
    storeIndex: {},
  })
  t.equal(typeof requestPackage, 'function')

  const pkgResponse = await requestPackage({alias: 'is-positive', pref: '1.0.0'}, {
    skipFetch: true,
    downloadPriority: 0,
    loggedPkg: {
      rawSpec: 'is-positive@1.0.0',
    },
    prefix: tempy.directory(),
    registry,
    verifyStoreIntegrity: true,
    preferredVersions: {},
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
    storePath: '.store',
    storeIndex: {},
  })
  t.equal(typeof requestPackage, 'function')

  const pkgResponse = await requestPackage({alias: 'is-positive', pref: '1.0.0'}, {
    currentPkgId: 'registry.npmjs.org/is-positive/1.0.0',
    update: false,
    skipFetch: true,
    downloadPriority: 0,
    loggedPkg: {
      rawSpec: 'is-positive@1.0.0',
    },
    prefix: tempy.directory(),
    registry,
    verifyStoreIntegrity: true,
    preferredVersions: {},
    shrinkwrapResolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
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
  const tarballPath = path.relative(prefix, path.join(__dirname, 'pnpm-package-requester-0.8.1.tgz'))
  const tarball = `file:${tarballPath}`
  const pkgId = `file:${encodeURIComponent(tarball)}`
  const wantedPackage = {alias: 'is-positive', pref: '1.0.0'}
  const storePath = '.store'
  const requestPackageOpts = {
    currentPkgId: pkgId,
    downloadPriority: 0,
    verifyStoreIntegrity: true,
    preferredVersions: {},
    update: false,
    loggedPkg: {
      rawSpec: tarball,
    },
    prefix,
    registry,
  }

  {
    const fakeResolve = () => Promise.resolve({
      id: pkgId,
      resolution: {
        integrity: 'sha1-BBBBBBBBBBBBBBBBBBBBBBBBBBB=',
        tarball,
      },
      resolvedVia: 'npm-registry',
    })
    const requestPackage = createPackageRequester(fakeResolve, fetch, {
      storePath,
      storeIndex: {},
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      shrinkwrapResolution: {
        integrity: 'sha1-BBBBBBBBBBBBBBBBBBBBBBBBBBB=',
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

    // the second time we request the package, fromStore should be true
    const response2 = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      shrinkwrapResolution: {
        integrity: 'sha1-BBBBBBBBBBBBBBBBBBBBBBBBBBB=',
        tarball,
      },
    }) as PackageResponse & {
      fetchingFiles: Promise<PackageFilesResponse>,
      finishing: Promise<void>,
    }
    await response2.fetchingFiles
    await response2.finishing

    t.ok((await response2.fetchingFiles).fromStore, 'correctly update fromStore after we downloaded it')
  }

  {
    const fakeResolve = () => Promise.resolve({
      id: pkgId,
      resolution: {
        integrity: 'sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=',
        tarball,
      },
      resolvedVia: 'npm-registry',
    })
    const requestPackage = createPackageRequester(fakeResolve, fetch, {
      storePath,
      storeIndex: {
        [pkgIdToFilename(pkgId)]: [] as string[],
      },
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      shrinkwrapResolution: {
        integrity: 'sha1-BBBBBBBBBBBBBBBBBBBBBBBBBBB=',
        tarball,
      },
    }) as PackageResponse & {fetchingFiles: Promise<PackageFilesResponse>, finishing: Promise<void>}
    await response.fetchingFiles
    await response.finishing

    t.ok(response.body.updated === true)
    t.notOk((await response.fetchingFiles).fromStore, 're-unpack tarball if its integrity has changed')
  }

  {
    const fakeResolve = () => Promise.resolve({
      id: pkgId,
      resolution: {
        integrity: 'sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=',
        tarball,
      },
      resolvedVia: 'npm-registry',
    })
    const requestPackage = createPackageRequester(fakeResolve, fetch, {
      storePath,
      storeIndex: {
        [pkgIdToFilename(pkgId)]: [] as string[],
      },
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      shrinkwrapResolution: {
        integrity: 'sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=',
        tarball,
      },
    }) as PackageResponse & {fetchingFiles: Promise<PackageFilesResponse>, finishing: Promise<void>}
    await response.fetchingFiles
    await response.finishing

    t.ok(response.body.updated === false)
    t.ok((await response.fetchingFiles).fromStore, 'use existing package from store if integrities matched')
  }

  t.end()
})

test('fetchPackageToStore()', async (t) => {
  const packageRequester = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storePath: '.store',
    storeIndex: {},
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const storePath = '.store'
  const fetchResult = await packageRequester.fetchPackageToStore({
    force: false,
    pkgId,
    prefix: tempy.directory(),
    verifyStoreIntegrity: true,
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    }
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
    verifyStoreIntegrity: true,
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    }
  })

  // This verifies that when a package has been cached with no full manifest
  // the full manifest is requested and added to the cache
  t.ok((await fetchResult2.fetchingRawManifest)!.name, 'full manifest returned')

  t.end()
})

test('fetchPackageToStore() concurrency check', async (t) => {
  const packageRequester = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storePath: '.store',
    storeIndex: {},
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const storePath = '.store'
  const prefix1 = tempy.directory()
  const prefix2 = tempy.directory()
  const fetchResults = await Promise.all([
    packageRequester.fetchPackageToStore({
      force: false,
      pkgId,
      prefix: prefix1,
      verifyStoreIntegrity: true,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      }
    }),
    packageRequester.fetchPackageToStore({
      force: false,
      pkgId,
      prefix: prefix2,
      verifyStoreIntegrity: true,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      }
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
  const tarballPath = path.join(__dirname, 'is-positive-1.0.0.tgz')

  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .reply(404)

  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .replyWithFile(200, tarballPath)

  const noRetryFetch = createFetcher({
    alwaysAuth: false,
    registry: 'https://registry.npmjs.org/',
    strictSsl: false,
    rawNpmConfig,
    fetchRetries: 0,
  })

  const packageRequester = createPackageRequester(resolve, noRetryFetch, {
    networkConcurrency: 1,
    storePath: tempy.directory(),
    storeIndex: {},
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'

  try {
    const badRequest = await packageRequester.fetchPackageToStore({
      force: false,
      pkgId,
      prefix: tempy.directory(),
      verifyStoreIntegrity: true,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      }
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
    verifyStoreIntegrity: true,
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    }
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
    storePath: '.store',
    storeIndex: {},
  })
  t.equal(typeof requestPackage, 'function')
  const prefix = tempy.directory()

  {
    const pkgResponse = await requestPackage({alias: 'is-positive', pref: '1.0.0'}, {
      downloadPriority: 0,
      loggedPkg: {
        rawSpec: 'is-positive@1.0.0',
      },
      prefix,
      registry,
      verifyStoreIntegrity: true,
      preferredVersions: {},
    }) as PackageResponse & {body: {manifest: {name: string}}}

    t.ok(pkgResponse.body, 'response has body')
    t.ok(pkgResponse.body.manifest.name, 'response has manifest')
  }

  {
    const pkgResponse = await requestPackage({alias: 'is-positive', pref: '1.0.0'}, {
      currentPkgId: 'registry.npmjs.org/is-positive/1.0.0',
      downloadPriority: 0,
      loggedPkg: {
        rawSpec: 'is-positive@1.0.0',
      },
      prefix,
      registry,
      verifyStoreIntegrity: true,
      preferredVersions: {},
      shrinkwrapResolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
    }) as PackageResponse & {fetchingRawManifest: Promise<PackageJson>}

    t.ok(pkgResponse.body, 'response has body')
    t.ok((await pkgResponse.fetchingRawManifest).name, 'response has manifest')
  }

  t.end()
})
