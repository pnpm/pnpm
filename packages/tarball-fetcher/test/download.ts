/// <reference path="../../../typings/index.d.ts" />
import createCafs from '@pnpm/cafs'
import { createFetchFromRegistry } from '@pnpm/fetch'
import createFetcher from '@pnpm/tarball-fetcher'
import path = require('path')
import cpFile = require('cp-file')
import fs = require('mz/fs')
import nock = require('nock')
import ssri = require('ssri')
import test = require('tape')
import tempy = require('tempy')

const cafsDir = tempy.directory()
console.log(cafsDir)
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

test('fail when tarball size does not match content-length', async t => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .times(2)
    .replyWithFile(200, tarballPath, {
      'Content-Length': (1024 * 1024).toString(),
    })

  process.chdir(tempy.directory())
  t.comment(`temp dir ${process.cwd()}`)

  const resolution = {
    // Even though the integrity of the downloaded tarball
    // will not match this value, the error will be about
    // Content-Length mismatch,
    // which indicates bad network connection. (see https://github.com/pnpm/pnpm/issues/1235)
    integrity: 'sha1-HssnaJydJVE+rbzZFKc/VAi+enY=',
    tarball: `${registry}foo.tgz`,
  }

  try {
    await fetch.tarball(cafs, resolution, {
      lockfileDir: process.cwd(),
    })
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.message, 'Actual size (1279) of tarball (http://example.com/foo.tgz) did not match the one specified in \'Content-Length\' header (1048576)')
    t.equal(err['code'], 'ERR_PNPM_BAD_TARBALL_SIZE')
    t.equal(err['expectedSize'], 1048576)
    t.equal(err['receivedSize'], tarballSize)
    t.equal(err['attempts'], 2)

    t.ok(scope.isDone())
    t.end()
  }
})

test('retry when tarball size does not match content-length', async t => {
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
  t.comment(`testing in ${process.cwd()}`)

  const resolution = { tarball: 'http://example.com/foo.tgz' }

  const result = await fetch.tarball(cafs, resolution, {
    lockfileDir: process.cwd(),
  })

  t.ok(result.filesIndex)
  t.ok(nock.isDone())
  t.end()
})

test('fail when integrity check fails two times in a row', async t => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .times(2)
    .replyWithFile(200, path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  try {
    await fetch.tarball(cafs, resolution, {
      lockfileDir: process.cwd(),
    })
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.message, 'Got unexpected checksum for "http://example.com/foo.tgz". Wanted "sha1-HssnaJydJVE+rbyZFKc/VAi+enY=". ' +
      'Got "sha512-VuFL1iPaIxJK/k3gTxStIkc6+wSiDwlLdnCWNZyapsVLobu/0onvGOZolASZpfBFiDJYrOIGiDzgLIULTW61Vg== sha1-ACjKMFA7S6uRFXSDFfH4aT+4B4Y=".')
    t.equal(err['code'], 'ERR_PNPM_TARBALL_INTEGRITY')
    t.equal(err['resource'], 'http://example.com/foo.tgz')
    t.equal(err['attempts'], 2)

    t.ok(scope.isDone())
    t.end()
  }
})

test('retry when integrity check fails', async t => {
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
  t.comment(`testing in ${process.cwd()}`)

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

  t.deepEqual(params[0], [1194, 1])
  t.deepEqual(params[1], [tarballSize, 2])

  t.ok(scope.isDone())
  t.end()
})

test('fail when integrity check of local file fails', async (t) => {
  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  await cpFile(
    path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'),
    path.resolve('tar.tgz')
  )
  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'file:tar.tgz',
  }

  let err: Error | null = null
  try {
    await fetch.tarball(cafs, resolution, {
      lockfileDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }

  t.ok(err, 'error thrown')
  t.equal(err.message, 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY= integrity checksum failed when using sha1: ' +
    'wanted sha1-HssnaJydJVE+rbyZFKc/VAi+enY= but got sha512-VuFL1iPaIxJK/k3gTxStIkc6+wSiDwlLdnCWNZyapsVLobu/0onvGOZolASZpfBFiDJYrOIGiDzgLIULTW61Vg== sha1-ACjKMFA7S6uRFXSDFfH4aT+4B4Y=. (1194 bytes)')
  t.equal(err['code'], 'EINTEGRITY')
  t.equal(err['resource'], path.resolve('tar.tgz'))
  t.equal(err['attempts'], 1)

  t.end()
})

test("don't fail when integrity check of local file succeeds", async (t) => {
  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

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

  t.equal(typeof filesIndex['package.json'], 'object', 'files index returned')

  t.end()
})

test("don't fail when fetching a local tarball in offline mode", async (t) => {
  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

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

  t.equal(typeof filesIndex['package.json'], 'object', 'files index returned')

  t.end()
})

test('fail when trying to fetch a non-local tarball in offline mode', async (t) => {
  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const tarballAbsoluteLocation = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const resolution = {
    integrity: await getFileIntegrity(tarballAbsoluteLocation),
    tarball: `${registry}foo.tgz`,
  }

  let err!: Error
  try {
    const fetch = createFetcher(fetchFromRegistry, getCredentials, {
      offline: true,
      retry: {
        maxTimeout: 100,
        minTimeout: 0,
        retries: 1,
      },
    })
    await fetch.tarball(cafs, resolution, {
      lockfileDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  t.equal(err['code'], 'ERR_PNPM_NO_OFFLINE_TARBALL')

  t.end()
})

test('retry on server error', async t => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .reply(500)
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  const index = await fetch.tarball(cafs, resolution, {
    lockfileDir: process.cwd(),
  })

  t.ok(index)

  t.ok(scope.isDone())
  t.end()
})

test('throw error when accessing private package w/o authorization', async t => {
  const scope = nock(registry)
    .get('/foo.tgz')
    .reply(403)

  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  let err!: Error

  try {
    await fetch.tarball(cafs, resolution, {
      lockfileDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  err = err || new Error()
  t.equal(err.message, 'GET http://example.com/foo.tgz: Forbidden - 403')
  t.equal(err['hint'], 'No authorization header was set for the request.')
  t.equal(err['code'], 'ERR_PNPM_FETCH_403')
  t.equal(err['request']['url'], 'http://example.com/foo.tgz')

  t.ok(scope.isDone())
  t.end()
})

test('accessing private packages', async t => {
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
  t.comment(`testing in ${process.cwd()}`)

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

  t.ok(index)

  t.ok(scope.isDone())
  t.end()
})

async function getFileIntegrity (filename: string) {
  return (await ssri.fromStream(fs.createReadStream(filename))).toString()
}
