/// <reference path="../../../__typings__/index.d.ts" />
import { jest } from '@jest/globals'
import fs from 'fs'
import path from 'path'
import { FetchError, PnpmError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { fixtures } from '@pnpm/test-fixtures'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { MockAgent, setGlobalDispatcher, getGlobalDispatcher, type Dispatcher } from 'undici'
import ssri from 'ssri'
import { temporaryDirectory } from 'tempy'

const originalModule = await import('@pnpm/logger')

jest.unstable_mockModule('@pnpm/logger', async () => {
  return {
    ...originalModule,
    globalWarn: jest.fn(),
  }
})

const { globalWarn } = await import('@pnpm/logger')
const {
  createTarballFetcher,
  BadTarballError,
  TarballIntegrityError,
} = await import('@pnpm/tarball-fetcher')

let mockAgent: MockAgent
let originalDispatcher: Dispatcher

beforeAll(() => {
  originalDispatcher = getGlobalDispatcher()
})

beforeEach(() => {
  jest.mocked(globalWarn).mockClear()
  mockAgent = new MockAgent()
  mockAgent.disableNetConnect()
  setGlobalDispatcher(mockAgent)
})

afterEach(async () => {
  await mockAgent.close()
})

afterAll(() => {
  setGlobalDispatcher(originalDispatcher)
})

const storeDir = temporaryDirectory()
const filesIndexFile = path.join(storeDir, 'index.json')
const cafs = createCafsStore(storeDir)

const f = fixtures(import.meta.dirname)
const tarballPath = f.find('babel-helper-hoist-variables-6.24.1.tgz')
const tarballSize = 1279
const tarballIntegrity = 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY='
const registry = 'http://example.com'
const fetchFromRegistry = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const fetch = createTarballFetcher(fetchFromRegistry, getAuthHeader, {
  rawConfig: {},
  retry: {
    maxTimeout: 100,
    minTimeout: 0,
    retries: 1,
  },
})
const pkg = {}

test('fail when tarball size does not match content-length', async () => {
  const tarballContent = fs.readFileSync(tarballPath)
  const mockPool = mockAgent.get(registry)

  // First request
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, tarballContent, {
    headers: { 'Content-Length': (1024 * 1024).toString() },
  })
  // Retry request
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, tarballContent, {
    headers: { 'Content-Length': (1024 * 1024).toString() },
  })

  process.chdir(temporaryDirectory())

  const resolution = {
    // Even though the integrity of the downloaded tarball
    // will not match this value, the error will be about
    // Content-Length mismatch,
    // which indicates bad network connection. (see https://github.com/pnpm/pnpm/issues/1235)
    integrity: 'sha1-HssnaJydJVE+rbzZFKc/VAi+enY=',
    tarball: `${registry}/foo.tgz`,
  }

  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
      pkg,
    })
  ).rejects.toThrow(
    new BadTarballError({
      expectedSize: 1048576,
      receivedSize: tarballSize,
      tarballUrl: resolution.tarball,
    })
  )
})

test('retry when tarball size does not match content-length', async () => {
  const tarballContent = fs.readFileSync(tarballPath)
  const mockPool = mockAgent.get(registry)

  // First request with wrong content-length
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, tarballContent, {
    headers: { 'Content-Length': (1024 * 1024).toString() },
  })
  // Retry with correct content-length
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, tarballContent, {
    headers: { 'Content-Length': tarballSize.toString() },
  })

  process.chdir(temporaryDirectory())

  const resolution = { tarball: `${registry}/foo.tgz` }

  const result = await fetch.remoteTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })

  expect(result.filesMap).toBeTruthy()
})

test('fail when integrity check fails two times in a row', async () => {
  const wrongTarball = f.find('babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const wrongTarballContent = fs.readFileSync(wrongTarball)
  const mockPool = mockAgent.get(registry)

  // Both requests return wrong tarball
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, wrongTarballContent, {
    headers: { 'Content-Length': '1194' },
  })
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, wrongTarballContent, {
    headers: { 'Content-Length': '1194' },
  })

  process.chdir(temporaryDirectory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: `${registry}/foo.tgz`,
  }

  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
      pkg,
    })
  ).rejects.toThrow(
    new TarballIntegrityError({
      algorithm: 'sha512',
      expected: 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY=',
      found: 'sha1-ACjKMFA7S6uRFXSDFfH4aT+4B4Y=',
      sri: '',
      url: resolution.tarball,
    })
  )
})

test('retry when integrity check fails', async () => {
  const wrongTarball = f.find('babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const wrongTarballContent = fs.readFileSync(wrongTarball)
  const tarballContent = fs.readFileSync(tarballPath)
  const mockPool = mockAgent.get(registry)

  // First request returns wrong tarball
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, wrongTarballContent, {
    headers: { 'Content-Length': '1194' },
  })
  // Retry returns correct tarball
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, tarballContent, {
    headers: { 'Content-Length': tarballSize.toString() },
  })

  process.chdir(temporaryDirectory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: `${registry}/foo.tgz`,
  }

  const params: Array<[number | null, number]> = []
  await fetch.remoteTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    onStart (size, attempts) {
      params.push([size, attempts])
    },
    pkg,
  })

  expect(params[0]).toStrictEqual([1194, 1])
  expect(params[1]).toStrictEqual([tarballSize, 2])
})

test('fail when integrity check of local file fails', async () => {
  const storeDir = temporaryDirectory()
  process.chdir(storeDir)

  f.copy('babel-helper-hoist-variables-7.0.0-alpha.10.tgz', 'tar.tgz')
  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'file:tar.tgz',
  }

  await expect(
    fetch.localTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
      pkg,
    })
  ).rejects.toThrow(
    new TarballIntegrityError({
      algorithm: 'sha512',
      expected: 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY=',
      found: 'sha1-ACjKMFA7S6uRFXSDFfH4aT+4B4Y=',
      sri: '',
      url: path.join(storeDir, 'tar.tgz'),
    })
  )
})

test("don't fail when integrity check of local file succeeds", async () => {
  process.chdir(temporaryDirectory())

  const localTarballLocation = path.resolve('tar.tgz')
  f.copy('babel-helper-hoist-variables-7.0.0-alpha.10.tgz', localTarballLocation)
  const resolution = {
    integrity: await getFileIntegrity(localTarballLocation),
    tarball: 'file:tar.tgz',
  }

  const { filesMap } = await fetch.localTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })

  expect(typeof filesMap.get('package.json')).toBe('string')
})

test("don't fail when fetching a local tarball in offline mode", async () => {
  process.chdir(temporaryDirectory())

  const tarballAbsoluteLocation = f.find('babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const resolution = {
    integrity: await getFileIntegrity(tarballAbsoluteLocation),
    tarball: `file:${tarballAbsoluteLocation}`,
  }

  const fetch = createTarballFetcher(fetchFromRegistry, getAuthHeader, {
    offline: true,
    rawConfig: {},
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })
  const { filesMap } = await fetch.localTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })

  expect(typeof filesMap.get('package.json')).toBe('string')
})

test('fail when trying to fetch a non-local tarball in offline mode', async () => {
  process.chdir(temporaryDirectory())

  const tarballAbsoluteLocation = f.find('babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const resolution = {
    integrity: await getFileIntegrity(tarballAbsoluteLocation),
    tarball: `${registry}/foo.tgz`,
  }

  const fetch = createTarballFetcher(fetchFromRegistry, getAuthHeader, {
    offline: true,
    rawConfig: {},
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })
  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
      pkg,
    })
  ).rejects.toThrow(
    new PnpmError('NO_OFFLINE_TARBALL',
      `A package is missing from the store but cannot download it in offline mode. \
The missing package may be downloaded from ${resolution.tarball}.`)
  )
})

test('retry on server error', async () => {
  const tarballContent = fs.readFileSync(tarballPath)
  const mockPool = mockAgent.get(registry)

  // First request returns 500
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(500, 'Internal Server Error')
  // Retry returns success
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, tarballContent, {
    headers: { 'Content-Length': tarballSize.toString() },
  })

  process.chdir(temporaryDirectory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: `${registry}/foo.tgz`,
  }

  const index = await fetch.remoteTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })

  expect(index).toBeTruthy()
})

test('throw error when accessing private package w/o authorization', async () => {
  const mockPool = mockAgent.get(registry)
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(403, 'Forbidden')

  process.chdir(temporaryDirectory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: `${registry}/foo.tgz`,
  }

  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
      pkg,
    })
  ).rejects.toThrow(
    new FetchError(
      {
        url: resolution.tarball,
      },
      {
        status: 403,
        statusText: 'Forbidden',
      }
    )
  )
})

test('do not retry when package does not exist', async () => {
  const mockPool = mockAgent.get(registry)
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(404, 'Not Found')

  process.chdir(temporaryDirectory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: `${registry}/foo.tgz`,
  }

  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
      pkg,
    })
  ).rejects.toThrow(
    new FetchError(
      {
        url: resolution.tarball,
      },
      {
        status: 404,
        statusText: 'Not Found',
      }
    )
  )
})

test('accessing private packages', async () => {
  const tarballContent = fs.readFileSync(tarballPath)
  const mockPool = mockAgent.get(registry)

  mockPool.intercept({
    path: '/foo.tgz',
    method: 'GET',
    headers: {
      authorization: 'Bearer ofjergrg349gj3f2',
    },
  }).reply(200, tarballContent, {
    headers: { 'Content-Length': tarballSize.toString() },
  })

  process.chdir(temporaryDirectory())

  const getAuthHeader = () => 'Bearer ofjergrg349gj3f2'
  const fetch = createTarballFetcher(fetchFromRegistry, getAuthHeader, {
    rawConfig: {},
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })

  const resolution = {
    integrity: tarballIntegrity,
    registry: `${registry}/`,
    tarball: `${registry}/foo.tgz`,
  }

  const index = await fetch.remoteTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })

  expect(index).toBeTruthy()
})

async function getFileIntegrity (filename: string) {
  return (await ssri.fromStream(fs.createReadStream(filename))).toString()
}

// Covers the regression reported in https://github.com/pnpm/pnpm/issues/4064
test('fetch a big repository', async () => {
  // Enable network for this test
  mockAgent.enableNetConnect(/codeload\.github\.com/)

  process.chdir(temporaryDirectory())

  const resolution = { tarball: 'https://codeload.github.com/sveltejs/action-deploy-docs/tar.gz/a65fbf5a90f53c9d72fed4daaca59da50f074355' }

  const result = await fetch.gitHostedTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })

  expect(result.filesMap).toBeTruthy()
})

test('fail when preparing a git-hosted package', async () => {
  // Enable network for this test
  mockAgent.enableNetConnect(/codeload\.github\.com/)

  process.chdir(temporaryDirectory())

  const resolution = { tarball: 'https://codeload.github.com/pnpm-e2e/prepare-script-fails/tar.gz/ba58874aae1210a777eb309dd01a9fdacc7e54e7' }

  await expect(
    fetch.gitHostedTarball(cafs, resolution, {
      allowBuild: (pkgName) => pkgName === '@pnpm.e2e/prepare-script-fails',
      filesIndexFile,
      lockfileDir: process.cwd(),
      pkg,
    })
  ).rejects.toThrow('Failed to prepare git-hosted package fetched from "https://codeload.github.com/pnpm-e2e/prepare-script-fails/tar.gz/ba58874aae1210a777eb309dd01a9fdacc7e54e7": @pnpm.e2e/prepare-script-fails@1.0.0 npm-install: `npm install`')
})

test('take only the files included in the package, when fetching a git-hosted package', async () => {
  // Enable network for this test
  mockAgent.enableNetConnect(/codeload\.github\.com/)

  process.chdir(temporaryDirectory())

  const resolution = { tarball: 'https://codeload.github.com/pnpm-e2e/pkg-with-ignored-files/tar.gz/958d6d487217512bb154d02836e9b5b922a600d8' }

  const result = await fetch.gitHostedTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })

  expect(Array.from(result.filesMap.keys()).sort(lexCompare)).toStrictEqual([
    'README.md',
    'dist/index.js',
    'package.json',
  ])
})

test('fail when extracting a broken tarball', async () => {
  const mockPool = mockAgent.get(registry)

  // Both requests return invalid tarball content
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, 'this is not a valid tarball')
  mockPool.intercept({ path: '/foo.tgz', method: 'GET' }).reply(200, 'this is not a valid tarball')

  process.chdir(temporaryDirectory())

  const resolution = {
    tarball: `${registry}/foo.tgz`,
  }

  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
      pkg,
    })
  ).rejects.toThrow(`Failed to add tarball from "${registry}/foo.tgz" to store: Invalid checksum for TAR header at offset 0. Expected 0, got NaN`
  )
})

test('do not build the package when scripts are ignored', async () => {
  // Enable network for this test
  mockAgent.enableNetConnect(/codeload\.github\.com/)

  process.chdir(temporaryDirectory())

  const tarball = 'https://codeload.github.com/pnpm-e2e/prepare-script-works/tar.gz/55416a9c468806a935636c0ad0371a14a64df8c9'
  const resolution = { tarball }

  const fetch = createTarballFetcher(fetchFromRegistry, getAuthHeader, {
    ignoreScripts: true,
    rawConfig: {},
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })
  const { filesMap } = await fetch.gitHostedTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })

  expect(filesMap.has('package.json')).toBeTruthy()
  expect(filesMap.has('prepare.txt')).toBeFalsy()
  expect(globalWarn).toHaveBeenCalledWith(`The git-hosted package fetched from "${tarball}" has to be built but the build scripts were ignored.`)
})

test('when extracting files with the same name, pick the last ones', async () => {
  const tar = f.find('tarball-with-duplicate-files/archive.tar')
  const resolution = {
    tarball: `file:${tar}`,
  }

  const { filesMap, manifest } = await fetch.localTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    readManifest: true,
    pkg,
  })
  const pkgJson = JSON.parse(fs.readFileSync(filesMap.get('package.json')!, 'utf8'))
  expect(pkgJson.name).toBe('pkg2')
  expect(manifest?.name).toBe('pkg2')
})

test('use the subfolder when path is present', async () => {
  // Enable network for this test
  mockAgent.enableNetConnect(/codeload\.github\.com/)

  process.chdir(temporaryDirectory())

  const resolution = {
    tarball: 'https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/2b42a57a945f19f8ffab8ecbd2021fdc2c58ee22',
    path: '/packages/simple-react-app',
  }

  const fetch = createTarballFetcher(fetchFromRegistry, getAuthHeader, {
    ignoreScripts: true,
    rawConfig: {},
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })
  const { filesMap } = await fetch.gitHostedTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })

  expect(filesMap.has('package.json')).toBeTruthy()
  expect(filesMap.has('lerna.json')).toBeFalsy()
})

test('prevent directory traversal attack when path is present', async () => {
  // Enable network for this test
  mockAgent.enableNetConnect(/codeload\.github\.com/)

  process.chdir(temporaryDirectory())

  const tarball = 'https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/2b42a57a945f19f8ffab8ecbd2021fdc2c58ee22'
  const path = '../../etc'
  const resolution = { tarball, path }

  const fetch = createTarballFetcher(fetchFromRegistry, getAuthHeader, {
    ignoreScripts: true,
    rawConfig: {},
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })

  await expect(() => fetch.gitHostedTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })).rejects.toThrow(`Failed to prepare git-hosted package fetched from "${tarball}": Path "${path}" should be a sub directory`)
})

test('fail when path is not exists', async () => {
  // Enable network for this test
  mockAgent.enableNetConnect(/codeload\.github\.com/)

  process.chdir(temporaryDirectory())

  const tarball = 'https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/2b42a57a945f19f8ffab8ecbd2021fdc2c58ee22'
  const path = '/not-exists'
  const resolution = { tarball, path }

  const fetch = createTarballFetcher(fetchFromRegistry, getAuthHeader, {
    ignoreScripts: true,
    rawConfig: {},
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })

  await expect(() => fetch.gitHostedTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    pkg,
  })).rejects.toThrow(`Failed to prepare git-hosted package fetched from "${tarball}": Path "${path}" is not a directory`)
})
