/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'fs'
import path from 'path'
import { FetchError, PnpmError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { globalWarn } from '@pnpm/logger'
import { fixtures } from '@pnpm/test-fixtures'
import {
  createTarballFetcher,
  BadTarballError,
  TarballIntegrityError,
} from '@pnpm/tarball-fetcher'
import { type DependencyManifest } from '@pnpm/types'
import nock from 'nock'
import safePromiseDefer from 'safe-promise-defer'
import ssri from 'ssri'
import tempy from 'tempy'

jest.mock('@pnpm/logger', () => {
  const originalModule = jest.requireActual('@pnpm/logger')
  return {
    ...originalModule,
    globalWarn: jest.fn(),
  }
})

beforeEach(() => {
  ;(globalWarn as jest.Mock).mockClear()
})

const cafsDir = tempy.directory()
const filesIndexFile = path.join(cafsDir, 'index.json')
const cafs = createCafsStore(cafsDir)

const f = fixtures(__dirname)
const tarballPath = f.find('babel-helper-hoist-variables-6.24.1.tgz')
const tarballSize = 1279
const tarballIntegrity = 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY='
const registry = 'http://example.com/'
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

test('fail when tarball size does not match content-length', async () => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .times(2)
    .replyWithFile(200, tarballPath, {
      'Content-Length': (1024 * 1024).toString(),
    })

  process.chdir(tempy.directory())

  const resolution = {
    // Even though the integrity of the downloaded tarball
    // will not match this value, the error will be about
    // Content-Length mismatch,
    // which indicates bad network connection. (see https://github.com/pnpm/pnpm/issues/1235)
    integrity: 'sha1-HssnaJydJVE+rbzZFKc/VAi+enY=',
    tarball: `${registry}foo.tgz`,
  }

  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
    })
  ).rejects.toThrow(
    new BadTarballError({
      expectedSize: 1048576,
      receivedSize: tarballSize,
      tarballUrl: resolution.tarball,
    })
  )
  expect(scope.isDone()).toBeTruthy()
})

test('retry when tarball size does not match content-length', async () => {
  nock(registry)
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': (1024 * 1024).toString(),
    })

  nock(registry)
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  process.chdir(tempy.directory())

  const resolution = { tarball: 'http://example.com/foo.tgz' }

  const result = await fetch.remoteTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
  })

  expect(result.filesIndex).toBeTruthy()
  expect(nock.isDone()).toBeTruthy()
})

test('fail when integrity check fails two times in a row', async () => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .times(2)
    .replyWithFile(200, f.find('babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  process.chdir(tempy.directory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
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
  expect(scope.isDone()).toBeTruthy()
})

test('retry when integrity check fails', async () => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .replyWithFile(200, f.find('babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  process.chdir(tempy.directory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  const params: Array<[number | null, number]> = []
  await fetch.remoteTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    onStart (size, attempts) {
      params.push([size, attempts])
    },
  })

  expect(params[0]).toStrictEqual([1194, 1])
  expect(params[1]).toStrictEqual([tarballSize, 2])

  expect(scope.isDone()).toBeTruthy()
})

test('fail when integrity check of local file fails', async () => {
  const storeDir = tempy.directory()
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
  process.chdir(tempy.directory())

  const localTarballLocation = path.resolve('tar.tgz')
  f.copy('babel-helper-hoist-variables-7.0.0-alpha.10.tgz', localTarballLocation)
  const resolution = {
    integrity: await getFileIntegrity(localTarballLocation),
    tarball: 'file:tar.tgz',
  }

  const { filesIndex } = await fetch.localTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
  })

  expect(typeof filesIndex['package.json']).toBe('string')
})

test("don't fail when fetching a local tarball in offline mode", async () => {
  process.chdir(tempy.directory())

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
  const { filesIndex } = await fetch.localTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
  })

  expect(typeof filesIndex['package.json']).toBe('string')
})

test('fail when trying to fetch a non-local tarball in offline mode', async () => {
  process.chdir(tempy.directory())

  const tarballAbsoluteLocation = f.find('babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const resolution = {
    integrity: await getFileIntegrity(tarballAbsoluteLocation),
    tarball: `${registry}foo.tgz`,
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
    })
  ).rejects.toThrow(
    new PnpmError('NO_OFFLINE_TARBALL',
      `A package is missing from the store but cannot download it in offline mode. \
The missing package may be downloaded from ${resolution.tarball}.`)
  )
})

test('retry on server error', async () => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .reply(500)
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  process.chdir(tempy.directory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  const index = await fetch.remoteTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
  })

  expect(index).toBeTruthy()

  expect(scope.isDone()).toBeTruthy()
})

test('throw error when accessing private package w/o authorization', async () => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .reply(403)

  process.chdir(tempy.directory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
    })
  ).rejects.toThrow(
    new FetchError(
      {
        url: resolution.tarball,
      },
      {
        status: 403,
        // statusText: 'Forbidden',
        statusText: '',
      }
    )
  )
  expect(scope.isDone()).toBeTruthy()
})

test('accessing private packages', async () => {
  const scope = nock(
    registry,
    {
      reqheaders: {
        authorization: 'Bearer ofjergrg349gj3f2',
      },
    }
  )
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  process.chdir(tempy.directory())

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
    registry,
    tarball: 'http://example.com/foo.tgz',
  }

  const index = await fetch.remoteTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
  })

  expect(index).toBeTruthy()

  expect(scope.isDone()).toBeTruthy()
})

async function getFileIntegrity (filename: string) {
  return (await ssri.fromStream(fs.createReadStream(filename))).toString()
}

// Covers the regression reported in https://github.com/pnpm/pnpm/issues/4064
test('fetch a big repository', async () => {
  process.chdir(tempy.directory())

  const resolution = { tarball: 'https://codeload.github.com/sveltejs/action-deploy-docs/tar.gz/a65fbf5a90f53c9d72fed4daaca59da50f074355' }

  const result = await fetch.gitHostedTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
  })

  expect(result.filesIndex).toBeTruthy()
})

test('fail when preparing a git-hosted package', async () => {
  process.chdir(tempy.directory())

  const resolution = { tarball: 'https://codeload.github.com/pnpm-e2e/prepare-script-fails/tar.gz/ba58874aae1210a777eb309dd01a9fdacc7e54e7' }

  await expect(
    fetch.gitHostedTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
    })
  ).rejects.toThrow('Failed to prepare git-hosted package fetched from "https://codeload.github.com/pnpm-e2e/prepare-script-fails/tar.gz/ba58874aae1210a777eb309dd01a9fdacc7e54e7": @pnpm.e2e/prepare-script-fails@1.0.0 npm-install: `npm install`')
})

test('fail when extracting a broken tarball', async () => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .times(2)
    .reply(200, 'this is not a valid tarball')

  process.chdir(tempy.directory())

  const resolution = {
    tarball: `${registry}foo.tgz`,
  }

  await expect(
    fetch.remoteTarball(cafs, resolution, {
      filesIndexFile,
      lockfileDir: process.cwd(),
    })
  ).rejects.toThrow(`Failed to unpack the tarball from "${registry}foo.tgz": Error: Invalid checksum for TAR header at offset 0. Expected 0, got NaN`
  )
  expect(scope.isDone()).toBeTruthy()
})

test('do not build the package when scripts are ignored', async () => {
  process.chdir(tempy.directory())

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
  const { filesIndex } = await fetch.gitHostedTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
  })

  expect(filesIndex).toHaveProperty(['package.json'])
  expect(filesIndex).not.toHaveProperty(['prepare.txt'])
  expect(globalWarn).toHaveBeenCalledWith(`The git-hosted package fetched from "${tarball}" has to be built but the build scripts were ignored.`)
})

test('when extracting files with the same name, pick the last ones', async () => {
  const tar = f.find('tarball-with-duplicate-files/archive.tar')
  const resolution = {
    tarball: `file:${tar}`,
  }

  const manifest = safePromiseDefer<DependencyManifest | undefined>()
  const { filesIndex } = await fetch.localTarball(cafs, resolution, {
    filesIndexFile,
    lockfileDir: process.cwd(),
    manifest,
  })
  const pkgJson = JSON.parse(fs.readFileSync(filesIndex['package.json'], 'utf8'))
  expect(pkgJson.name).toBe('pkg2')
  expect((await manifest())?.name).toBe('pkg2')
})
