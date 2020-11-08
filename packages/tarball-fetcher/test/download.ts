/// <reference path="../../../typings/index.d.ts" />
import createCafs from '@pnpm/cafs'
import PnpmError, { FetchError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/fetch'
import createFetcher, {
  BadTarballError,
  TarballIntegrityError,
} from '@pnpm/tarball-fetcher'
import path = require('path')
import cpFile = require('cp-file')
import fs = require('mz/fs')
import nock = require('nock')
import ssri = require('ssri')
import tempy = require('tempy')

const cafsDir = tempy.directory()
const cafs = createCafs(cafsDir)

const tarballPath = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-6.24.1.tgz')
const tarballSize = 1279
const tarballIntegrity = 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY='
const registry = 'http://example.com/'
const fetchFromRegistry = createFetchFromRegistry({})
const getCredentials = () => ({ authHeaderValue: undefined, alwaysAuth: undefined })
const fetch = createFetcher(fetchFromRegistry, getCredentials, {
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
    fetch.tarball(cafs, resolution, {
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

  const result = await fetch.tarball(cafs, resolution, {
    lockfileDir: process.cwd(),
  })

  expect(result.filesIndex).toBeTruthy()
  expect(nock.isDone()).toBeTruthy()
})

test('fail when integrity check fails two times in a row', async () => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .times(2)
    .replyWithFile(200, path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  process.chdir(tempy.directory())

  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  await expect(
    fetch.tarball(cafs, resolution, {
      lockfileDir: process.cwd(),
    })
  ).rejects.toThrow(
    new TarballIntegrityError({
      algorithm: 'sha512',
      expected: 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY=',
      found: 'sha512-VuFL1iPaIxJK/k3gTxStIkc6+wSiDwlLdnCWNZyapsVLobu/0onvGOZolASZpfBFiDJYrOIGiDzgLIULTW61Vg== sha1-ACjKMFA7S6uRFXSDFfH4aT+4B4Y=',
      sri: '',
      url: resolution.tarball,
    })
  )
  expect(scope.isDone()).toBeTruthy()
})

test('retry when integrity check fails', async () => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .replyWithFile(200, path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
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
  await fetch.tarball(cafs, resolution, {
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

  await cpFile(
    path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'),
    path.resolve('tar.tgz')
  )
  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'file:tar.tgz',
  }

  await expect(
    fetch.tarball(cafs, resolution, {
      lockfileDir: process.cwd(),
    })
  ).rejects.toThrow(
    new TarballIntegrityError({
      algorithm: 'sha512',
      expected: 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY=',
      found: 'sha512-VuFL1iPaIxJK/k3gTxStIkc6+wSiDwlLdnCWNZyapsVLobu/0onvGOZolASZpfBFiDJYrOIGiDzgLIULTW61Vg== sha1-ACjKMFA7S6uRFXSDFfH4aT+4B4Y=',
      sri: '',
      url: path.join(storeDir, 'tar.tgz'),
    })
  )
})

test("don't fail when integrity check of local file succeeds", async () => {
  process.chdir(tempy.directory())

  const localTarballLocation = path.resolve('tar.tgz')
  await cpFile(
    path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'),
    localTarballLocation
  )
  const resolution = {
    integrity: await getFileIntegrity(localTarballLocation),
    tarball: 'file:tar.tgz',
  }

  const { filesIndex } = await fetch.tarball(cafs, resolution, {
    lockfileDir: process.cwd(),
  })

  expect(typeof filesIndex['package.json']).toBe('object')
})

test("don't fail when fetching a local tarball in offline mode", async () => {
  process.chdir(tempy.directory())

  const tarballAbsoluteLocation = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const resolution = {
    integrity: await getFileIntegrity(tarballAbsoluteLocation),
    tarball: `file:${tarballAbsoluteLocation}`,
  }

  const fetch = createFetcher(fetchFromRegistry, getCredentials, {
    offline: true,
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })
  const { filesIndex } = await fetch.tarball(cafs, resolution, {
    lockfileDir: process.cwd(),
  })

  expect(typeof filesIndex['package.json']).toBe('object')
})

test('fail when trying to fetch a non-local tarball in offline mode', async () => {
  process.chdir(tempy.directory())

  const tarballAbsoluteLocation = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const resolution = {
    integrity: await getFileIntegrity(tarballAbsoluteLocation),
    tarball: `${registry}foo.tgz`,
  }

  const fetch = createFetcher(fetchFromRegistry, getCredentials, {
    offline: true,
    retry: {
      maxTimeout: 100,
      minTimeout: 0,
      retries: 1,
    },
  })
  await expect(
    fetch.tarball(cafs, resolution, {
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

  const index = await fetch.tarball(cafs, resolution, {
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
    fetch.tarball(cafs, resolution, {
      lockfileDir: process.cwd(),
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

  const getCredentials = () => ({
    alwaysAuth: undefined,
    authHeaderValue: 'Bearer ofjergrg349gj3f2',
  })
  const fetch = createFetcher(fetchFromRegistry, getCredentials, {
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

  const index = await fetch.tarball(cafs, resolution, {
    lockfileDir: process.cwd(),
  })

  expect(index).toBeTruthy()

  expect(scope.isDone()).toBeTruthy()
})

async function getFileIntegrity (filename: string) {
  return (await ssri.fromStream(fs.createReadStream(filename))).toString()
}
