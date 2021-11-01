/// <reference path="../../../typings/index.d.ts" />
import { promisify } from 'util'
import { promises as fs, statSync } from 'fs'
import path from 'path'
import { getFilePathInCafs, PackageFilesIndex } from '@pnpm/cafs'
import createClient from '@pnpm/client'
import { streamParser } from '@pnpm/logger'
import createPackageRequester, { PackageFilesResponse, PackageResponse } from '@pnpm/package-requester'
import { createCafsStore } from '@pnpm/package-store'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { DependencyManifest } from '@pnpm/types'
import delay from 'delay'
import { depPathToFilename } from 'dependency-path'
import loadJsonFile from 'load-json-file'
import ncpCB from 'ncp'
import nock from 'nock'
import normalize from 'normalize-path'
import tempy from 'tempy'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}`
const IS_POSTIVE_TARBALL = path.join(__dirname, 'is-positive-1.0.0.tgz')
const ncp = promisify(ncpCB as any) // eslint-disable-line @typescript-eslint/no-explicit-any

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
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
      name: 'is-positive',
      version: '1.0.0',
      id: `localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`,
      resolution: {
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
      manifest: {name: string}
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
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
  await ncp(path.join(__dirname, 'pnpm-package-requester-0.8.1.tgz'), tarballPath)
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
        name: '@pnpm/package-requester',
        version: '0.8.1',
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

  await ncp(path.join(__dirname, 'pnpm-package-requester-4.1.2.tgz'), tarballPath)
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
        name: '@pnpm/package-requester',
        version: '0.8.1',
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
        name: '@pnpm/package-requester',
        version: '0.8.1',
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
  await ncp(path.join(__dirname, 'pnpm-package-requester-0.8.1.tgz'), tarballPath)
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

  await ncp(path.join(__dirname, 'pnpm-package-requester-4.1.2.tgz'), tarballPath)
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
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
          integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
          integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
        integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
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
    }) as PackageResponse & {body: {manifest: {name: string}}}

    expect(pkgResponse.body).toBeTruthy()
    expect(pkgResponse.body.manifest.name).toBeTruthy()
  }

  {
    const pkgResponse = await requestPackage({ alias: 'is-positive', pref: '1.0.0' }, {
      currentPkg: {
        name: 'is-positive',
        version: '1.0.0',
        id: `localhost+${REGISTRY_MOCK_PORT}/is-positive/1.0.0`,
        resolution: {
          integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
          registry: `http://localhost:${REGISTRY_MOCK_PORT}/`,
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
        },
      },
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
      registry,
    }) as PackageResponse & {bundledManifest: () => Promise<DependencyManifest>}

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
    message: `Refetching ${path.join(storeDir, depPathToFilename(pkgId, process.cwd()))} to store. It was either modified or had no integrity checksums`,
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
  const pkgResponse = await requestPackage({ alias: 'not-compatible-with-any-os', optional: true, pref: '*' }, {
    downloadPriority: 0,
    lockfileDir: projectDir,
    preferredVersions: {},
    projectDir,
    registry,
  })

  expect(pkgResponse).toBeTruthy()
  expect(pkgResponse.body).toBeTruthy()

  expect(pkgResponse.body.isInstallable).toBe(false)
  expect(pkgResponse.body.id).toBe(`localhost+${REGISTRY_MOCK_PORT}/not-compatible-with-any-os/1.0.0`)

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
    }) as PackageResponse & {body: {manifest: {name: string}}}

    expect(pkgResponse.body).toBeTruthy()
    expect(pkgResponse.body.manifest).toBeUndefined()
    expect(pkgResponse.body.isInstallable).toBeFalsy()
    expect(pkgResponse.body.id).toBe(`github.com/${repo}/${commit}`)
  }
})
