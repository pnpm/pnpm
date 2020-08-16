///<reference path="../../../typings/index.d.ts" />
import { getFilePathInCafs } from '@pnpm/cafs'
import createClient from '@pnpm/client'
import { streamParser } from '@pnpm/logger'
import createPackageRequester, { PackageFilesResponse, PackageResponse } from '@pnpm/package-requester'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { DependencyManifest } from '@pnpm/types'
import delay from 'delay'
import fs = require('mz/fs')
import ncpCB = require('ncp')
import nock = require('nock')
import normalize = require('normalize-path')
import path = require('path')
import sinon = require('sinon')
import test = require('tape')
import tempy = require('tempy')
import { promisify } from 'util'

const registry = 'https://registry.npmjs.org/'
const IS_POSTIVE_TARBALL = path.join(__dirname, 'is-positive-1.0.0.tgz')
const ncp = promisify(ncpCB as any) // tslint:disable-line:no-any

const authConfig = { registry }

const { resolve, fetchers } = createClient({
  authConfig,
  storeDir: '.store',
})

test('request package', async t => {
  const storeDir = tempy.directory()
  t.comment(storeDir)
  const requestPackage = createPackageRequester(resolve, fetchers, {
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })
  t.equal(typeof requestPackage, 'function')

  const projectDir = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
  }) as PackageResponse

  t.ok(pkgResponse, 'response received')
  t.ok(pkgResponse.body, 'response has body')

  t.equal(pkgResponse.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct package ID')
  t.equal(pkgResponse.body.resolvedVia, 'npm-registry', 'responded with correct resolvedVia')
  t.equal(pkgResponse.body.isLocal, false, 'package is not local')
  t.equal(typeof pkgResponse.body.latest, 'string', 'latest is returned')
  t.equal(pkgResponse.body.manifest.name, 'is-positive', 'package manifest returned')
  t.ok(!pkgResponse.body.normalizedPref, 'no normalizedPref returned')
  t.deepEqual(pkgResponse.body.resolution, {
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  }, 'resolution returned')

  const files = await pkgResponse.files!()
  t.deepEqual(Object.keys(files.filesIndex).sort(),
    ['package.json', 'index.js', 'license', 'readme.md'].sort())
  t.notOk(files.fromStore)

  t.ok(pkgResponse.finishing!())

  t.end()
})

test('request package but skip fetching', async t => {
  const requestPackage = createPackageRequester(resolve, fetchers, {
    networkConcurrency: 1,
    storeDir: '.store',
    verifyStoreIntegrity: true,
  })
  t.equal(typeof requestPackage, 'function')

  const projectDir = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
    skipFetch: true,
  }) as PackageResponse

  t.ok(pkgResponse, 'response received')
  t.ok(pkgResponse.body, 'response has body')

  t.equal(pkgResponse.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct package ID')
  t.equal(pkgResponse.body.isLocal, false, 'package is not local')
  t.equal(typeof pkgResponse.body.latest, 'string', 'latest is returned')
  t.equal(pkgResponse.body.manifest.name, 'is-positive', 'package manifest returned')
  t.ok(!pkgResponse.body.normalizedPref, 'no normalizedPref returned')
  t.deepEqual(pkgResponse.body.resolution, {
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  }, 'resolution returned')

  t.notOk(pkgResponse.files, 'files fetching not done')
  t.notOk(pkgResponse.finishing)

  t.end()
})

test('request package but skip fetching, when resolution is already available', async t => {
  const requestPackage = createPackageRequester(resolve, fetchers, {
    networkConcurrency: 1,
    storeDir: '.store',
    verifyStoreIntegrity: true,
  })
  t.equal(typeof requestPackage, 'function')

  const projectDir = tempy.directory()
  const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
    currentPackageId: 'registry.npmjs.org/is-positive/1.0.0',
    currentResolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
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
      latest: string,
      manifest: {name: string},
    },
    files: () => Promise<object>,
    finishing: () => Promise<void>,
  }

  t.ok(pkgResponse, 'response received')
  t.ok(pkgResponse.body, 'response has body')

  t.equal(pkgResponse.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct package ID')
  t.equal(pkgResponse.body.isLocal, false, 'package is not local')
  t.equal(typeof pkgResponse.body.latest, 'string', 'latest is returned')
  t.equal(pkgResponse.body.manifest.name, 'is-positive', 'package manifest returned')
  t.ok(!pkgResponse.body.normalizedPref, 'no normalizedPref returned')
  t.deepEqual(pkgResponse.body.resolution, {
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  }, 'resolution returned')

  t.notOk(pkgResponse.files, 'files fetching not done')
  t.notOk(pkgResponse.finishing)

  t.end()
})

test('refetch local tarball if its integrity has changed', async t => {
  const projectDir = tempy.directory()
  const tarballPath = path.join(projectDir, 'tarball.tgz')
  const tarballRelativePath = path.relative(projectDir, tarballPath)
  await ncp(path.join(__dirname, 'pnpm-package-requester-0.8.1.tgz'), tarballPath)
  const tarball = `file:${tarballRelativePath}`
  const wantedPackage = { pref: tarball }
  const storeDir = tempy.directory()
  const pkgId = `file:${normalize(tarballRelativePath)}`
  const requestPackageOpts = {
    currentPackageId: pkgId,
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
    skipFetch: true,
    update: false,
  }

  {
    const requestPackage = createPackageRequester(resolve, fetchers, {
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      currentResolution: {
        integrity: 'sha512-lqODmYcc/FKOGROEUByd5Sbugqhzgkv+Hij9PXH0sZVQsU2npTQ0x3L81GCtHilFKme8lhBtD31Vxg/AKYrAvg==',
        tarball,
      },
    }) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>,
      finishing: () => Promise<void>,
    }
    await response.files!()
    await response.finishing!()

    t.ok(response.body.updated === false, 'resolution not updated')
    t.notOk((await response.files!()).fromStore, 'unpack tarball if it is not in store yet')
    t.ok(await response.bundledManifest!())
  }

  await ncp(path.join(__dirname, 'pnpm-package-requester-4.1.2.tgz'), tarballPath)
  await delay(50)

  {
    const requestPackage = createPackageRequester(resolve, fetchers, {
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      currentResolution: {
        integrity: 'sha512-lqODmYcc/FKOGROEUByd5Sbugqhzgkv+Hij9PXH0sZVQsU2npTQ0x3L81GCtHilFKme8lhBtD31Vxg/AKYrAvg==',
        tarball,
      },
    }) as PackageResponse
    await response.files!()
    await response.finishing!()

    t.ok(response.body.updated === true, 'resolution updated')
    t.notOk((await response.files!()).fromStore, 'reunpack tarball if its integrity is not up-to-date')
    t.ok(await response.bundledManifest!())
  }

  {
    const requestPackage = createPackageRequester(resolve, fetchers, {
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, {
      ...requestPackageOpts,
      currentResolution: {
        integrity: 'sha512-v3uhYkN+Eh3Nus4EZmegjQhrfpdPIH+2FjrkeBc6ueqZJWWRaLnSYIkD0An6m16D3v+6HCE18ox6t95eGxj5Pw==',
        tarball,
      },
    }) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>,
      finishing: () => Promise<void>,
    }
    await response.files!()
    await response.finishing!()

    t.ok(response.body.updated === false, 'resolution not updated')
    t.ok((await response.files!()).fromStore, 'do not reunpack tarball if its integrity is up-to-date')
    t.ok(await response.bundledManifest!())
  }

  t.end()
})

test('refetch local tarball if its integrity has changed. The requester does not know the correct integrity', async t => {
  const projectDir = tempy.directory()
  const tarballPath = path.join(projectDir, 'tarball.tgz')
  await ncp(path.join(__dirname, 'pnpm-package-requester-0.8.1.tgz'), tarballPath)
  const tarball = `file:${tarballPath}`
  const wantedPackage = { pref: tarball }
  const storeDir = path.join(__dirname, '..', '.store')
  const requestPackageOpts = {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
    update: false,
  }

  {
    const requestPackage = createPackageRequester(resolve, fetchers, {
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>,
      finishing: () => Promise<void>,
    }
    await response.files!()
    await response.finishing!()

    t.ok(response.body.updated === true, 'resolution updated')
    t.notOk((await response.files!()).fromStore, 'unpack tarball if it is not in store yet')
    t.ok(await response.bundledManifest!())
  }

  await ncp(path.join(__dirname, 'pnpm-package-requester-4.1.2.tgz'), tarballPath)
  await delay(50)

  {
    const requestPackage = createPackageRequester(resolve, fetchers, {
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>,
      finishing: () => Promise<void>,
    }
    await response.files!()
    await response.finishing!()

    t.ok(response.body.updated === true, 'resolution updated')
    t.notOk((await response.files!()).fromStore, 'reunpack tarball if its integrity is not up-to-date')
    t.ok(await response.bundledManifest!())
  }

  {
    const requestPackage = createPackageRequester(resolve, fetchers, {
      storeDir,
      verifyStoreIntegrity: true,
    })

    const response = await requestPackage(wantedPackage, requestPackageOpts) as PackageResponse & {
      files: () => Promise<PackageFilesResponse>,
      finishing: () => Promise<void>,
    }
    await response.files()
    await response.finishing()

    t.ok((await response.files!()).fromStore, 'do not reunpack tarball if its integrity is up-to-date')
    t.ok(await response.bundledManifest!())
  }

  t.end()
})

test('fetchPackageToStore()', async (t) => {
  const packageRequester = createPackageRequester(resolve, fetchers, {
    networkConcurrency: 1,
    storeDir: tempy.directory(),
    verifyStoreIntegrity: true,
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const fetchResult = packageRequester.fetchPackageToStore({
    force: false,
    lockfileDir: tempy.directory(),
    pkgId,
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })

  t.notOk(fetchResult.bundledManifest, 'full manifest not returned')

  const files = await fetchResult.files()
  t.deepEqual(Object.keys(files.filesIndex).sort(),
    ['package.json', 'index.js', 'license', 'readme.md'].sort(),
    'returned info about files after fetch completed')
  t.notOk(files.fromStore)

  t.ok(fetchResult.finishing())

  const fetchResult2 = packageRequester.fetchPackageToStore({
    fetchRawManifest: true,
    force: false,
    lockfileDir: tempy.directory(),
    pkgId,
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })

  // This verifies that when a package has been cached with no full manifest
  // the full manifest is requested and added to the cache
  t.deepEqual(
    await fetchResult2.bundledManifest!(),
    {
      engines: { node: '>=0.10.0' },
      name: 'is-positive',
      scripts: { test: 'node test.js' },
      version: '1.0.0',
    },
    'full manifest returned'
  )

  t.end()
})

test('fetchPackageToStore() concurrency check', async (t) => {
  const storeDir = tempy.directory()
  const cafsDir = path.join(storeDir, 'files')
  const packageRequester = createPackageRequester(resolve, fetchers, {
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const projectDir1 = tempy.directory()
  const projectDir2 = tempy.directory()
  const fetchResults = await Promise.all([
    packageRequester.fetchPackageToStore({
      force: false,
      lockfileDir: projectDir1,
      pkgId,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
    }),
    packageRequester.fetchPackageToStore({
      force: false,
      lockfileDir: projectDir2,
      pkgId,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
    }),
  ])

  let ino1!: Number
  let ino2!: Number

  {
    const fetchResult = fetchResults[0]
    const files = await fetchResult.files()

    ino1 = fs.statSync(getFilePathInCafs(cafsDir, files.filesIndex['package.json'].integrity, 'nonexec')).ino

    t.deepEqual(Object.keys(files.filesIndex).sort(),
      ['package.json', 'index.js', 'license', 'readme.md'].sort(),
      'returned info about files after fetch completed'
    )
    t.notOk(files.fromStore)

    t.ok(fetchResult.finishing)
  }

  {
    const fetchResult = fetchResults[1]
    const files = await fetchResult.files()

    ino2 = fs.statSync(getFilePathInCafs(cafsDir, files.filesIndex['package.json'].integrity, 'nonexec')).ino

    t.deepEqual(Object.keys(files.filesIndex).sort(),
      ['package.json', 'index.js', 'license', 'readme.md'].sort(),
      'returned info about files after fetch completed'
    )
    t.notOk(files.fromStore)

    t.ok(fetchResult.finishing())
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

  const noRetry = createClient({
    authConfig,
    retry: { retries: 0 },
    storeDir: '.pnpm',
  })

  const packageRequester = createPackageRequester(noRetry.resolve, noRetry.fetchers, {
    networkConcurrency: 1,
    storeDir: tempy.directory(),
    verifyStoreIntegrity: true,
  })

  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'

  try {
    const badRequest = packageRequester.fetchPackageToStore({
      force: false,
      lockfileDir: tempy.directory(),
      pkgId,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
    })
    await badRequest.files()
    t.fail('first fetch should have failed')
  } catch (err) {
    t.pass('first fetch failed')
  }

  const fetchResult = packageRequester.fetchPackageToStore({
    force: false,
    lockfileDir: tempy.directory(),
    pkgId,
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })
  const files = await fetchResult.files()
  t.deepEqual(Object.keys(files.filesIndex).sort(),
    [ 'package.json', 'index.js', 'license', 'readme.md' ].sort(),
    'returned info about files after fetch completed'
  )
  t.notOk(files.fromStore)

  t.ok(fetchResult.finishing())
  t.ok(nock.isDone())

  t.end()
})

// This test was added to cover the issue described here: https://github.com/pnpm/supi/issues/65
test('always return a package manifest in the response', async t => {
  nock.cleanAll()
  const requestPackage = createPackageRequester(resolve, fetchers, {
    networkConcurrency: 1,
    storeDir: tempy.directory(),
    verifyStoreIntegrity: true,
  })
  t.equal(typeof requestPackage, 'function')
  const projectDir = tempy.directory()

  {
    const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
      registry,
    }) as PackageResponse & {body: {manifest: {name: string}}}

    t.ok(pkgResponse.body, 'response has body')
    t.ok(pkgResponse.body.manifest.name, 'response has manifest')
  }

  {
    const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
      currentPackageId: 'registry.npmjs.org/is-positive/1.0.0',
      currentResolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
      registry,
    }) as PackageResponse & {bundledManifest: () => Promise<DependencyManifest>}

    t.ok(pkgResponse.body, 'response has body')
    t.deepEqual(
      await pkgResponse.bundledManifest!(),
      {
        engines: { node: '>=0.10.0' },
        name: 'is-positive',
        scripts: { test: 'node test.js' },
        version: '1.0.0',
      },
      'response has manifest'
    )
  }

  t.end()
})

// Covers https://github.com/pnpm/pnpm/issues/1293
test('fetchPackageToStore() fetch raw manifest of cached package', async (t) => {
  nock(registry)
    .get('/is-positive/-/is-positive-1.0.0.tgz')
    .replyWithFile(200, IS_POSTIVE_TARBALL)

  const packageRequester = createPackageRequester(resolve, fetchers, {
    networkConcurrency: 1,
    storeDir: tempy.directory(),
    verifyStoreIntegrity: true,
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
      lockfileDir: tempy.directory(),
      pkgId,
      resolution,
    }),
    packageRequester.fetchPackageToStore({
      fetchRawManifest: true,
      force: false,
      lockfileDir: tempy.directory(),
      pkgId,
      resolution,
    }),
  ])

  t.ok(await fetchResults[1].bundledManifest!())
  t.end()
})

test('refetch package to store if it has been modified', async (t) => {
  nock.cleanAll()
  const storeDir = tempy.directory()
  const cafsDir = path.join(storeDir, 'files')
  const lockfileDir = tempy.directory()
  t.comment(`store location: ${storeDir}`)

  const pkgId = 'registry.npmjs.org/magic-hook/2.0.0'
  const resolution = {
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/magic-hook/-/magic-hook-2.0.0.tgz',
  }

  let indexJsFile!: string
  {
    const packageRequester = createPackageRequester(resolve, fetchers, {
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const fetchResult = packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      lockfileDir,
      pkgId,
      resolution,
    })

    const { filesIndex } = await fetchResult.files()
    indexJsFile = getFilePathInCafs(cafsDir, filesIndex['index.js'].integrity, 'nonexec')
  }

  // Adding some content to the file to change its integrity
  await fs.appendFile(indexJsFile, '// foobar')

  const reporter = sinon.spy()
  streamParser.on('data', reporter)

  {
    const packageRequester = createPackageRequester(resolve, fetchers, {
      networkConcurrency: 1,
      storeDir,
      verifyStoreIntegrity: true,
    })

    const fetchResult = packageRequester.fetchPackageToStore({
      fetchRawManifest: false,
      force: false,
      lockfileDir,
      pkgId,
      resolution,
    })

    await fetchResult.files()
  }

  streamParser.removeListener('data', reporter)

  t.notOk((await fs.readFile(indexJsFile, 'utf8')).includes('// foobar'))

  t.ok(reporter.calledWithMatch({
    level: 'warn',
    message: `Refetching ${path.join(storeDir, pkgIdToFilename(pkgId, process.cwd()))} to store. It was either modified or had no integrity checksums`,
    name: 'pnpm:package-requester',
    prefix: lockfileDir,
  }), 'refetch logged')

  t.end()
})
