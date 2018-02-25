import test = require('tape')
import createPackageRequester from '@pnpm/package-requester'
import createResolver from '@pnpm/npm-resolver'
import createFetcher from '@pnpm/tarball-fetcher'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import path = require('path')
import tempy = require('tempy')

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
  const requestPackage = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storePath: '.store',
    storeIndex: {},
  })
  t.equal(typeof requestPackage, 'function')

  const pkgResponse = await requestPackage({alias: 'is-positive', pref: '1.0.0'}, {
    downloadPriority: 0,
    loggedPkg: {},
    prefix: tempy.directory(),
    registry,
    verifyStoreIntegrity: true,
    preferredVersions: {},
  })

  t.ok(pkgResponse, 'response received')
  t.ok(pkgResponse.body, 'response has body')

  t.equal(pkgResponse.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct package ID')
  t.equal(pkgResponse.body.inStoreLocation, '.store/registry.npmjs.org/is-positive/1.0.0', 'package location in store returned')
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
    loggedPkg: {},
    prefix: tempy.directory(),
    registry,
    verifyStoreIntegrity: true,
    preferredVersions: {},
  })

  t.ok(pkgResponse, 'response received')
  t.ok(pkgResponse.body, 'response has body')

  t.equal(pkgResponse.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct package ID')
  t.equal(pkgResponse.body.inStoreLocation, '.store/registry.npmjs.org/is-positive/1.0.0', 'package location in store returned')
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
    loggedPkg: {},
    prefix: tempy.directory(),
    registry,
    verifyStoreIntegrity: true,
    preferredVersions: {},
    shrinkwrapResolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })

  t.ok(pkgResponse, 'response received')
  t.ok(pkgResponse.body, 'response has body')

  t.equal(pkgResponse.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct package ID')
  t.equal(pkgResponse.body.inStoreLocation, '.store/registry.npmjs.org/is-positive/1.0.0', 'package location in store returned')
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
    update: false,
    loggedPkg: {},
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
    })
    await response.fetchingFiles
    await response.finishing

    t.notOk((await response.fetchingFiles).fromStore, 'unpack tarball if it is not in store yet')
  }

  {
    const fakeResolve = () => Promise.resolve({
      id: pkgId,
      resolution: {
        integrity: 'sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=',
        tarball,
      },
    })
    const requestPackage = createPackageRequester(fakeResolve, fetch, {
      storePath,
      storeIndex: {
        [pkgIdToFilename(pkgId)]: true,
      },
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      shrinkwrapResolution: {
        integrity: 'sha1-BBBBBBBBBBBBBBBBBBBBBBBBBBB=',
        tarball,
      },
    })
    await response.fetchingFiles
    await response.finishing

    t.notOk((await response.fetchingFiles).fromStore, 're-unpack tarball if its integrity has changed')
  }

  {
    const fakeResolve = () => Promise.resolve({
      id: pkgId,
      resolution: {
        integrity: 'sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=',
        tarball,
      },
    })
    const requestPackage = createPackageRequester(fakeResolve, fetch, {
      storePath,
      storeIndex: {
        [pkgIdToFilename(pkgId)]: true,
      },
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      shrinkwrapResolution: {
        integrity: 'sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=',
        tarball,
      },
    })
    await response.fetchingFiles
    await response.finishing

    t.ok((await response.fetchingFiles).fromStore, 'use existing package from store if integrities matched')
  }

  t.end()
})
