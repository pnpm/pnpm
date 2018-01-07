import test = require('tape')
import nock = require('nock')
import createFetcher from '@pnpm/tarball-fetcher'
import path = require('path')
import tempy = require('tempy')

const tarballPath = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-6.24.1.tgz')
const tarballSize = 1279
const tarballIntegrity = 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY='
const registry = 'http://example.com/'
const fetch = createFetcher({
  registry,
  rawNpmConfig: {
    registry,
  },
  fetchRetries: 1,
  fetchRetryMintimeout: 0,
  fetchRetryMaxtimeout: 100,
})

test('fail when tarball size does not match content-length', async t => {
  nock(registry)
    .get('/foo.tgz')
    .times(2)
    .replyWithFile(200, tarballPath, {
      'Content-Length': (1024 * 1024).toString(),
    })

  process.chdir(tempy.directory())
  const unpackTo = path.resolve('unpacked')
  const cachedTarballLocation = path.resolve('cached')
  const resolution = { tarball: `${registry}foo.tgz` }

  try {
    await fetch.tarball(resolution, unpackTo, {
      cachedTarballLocation,
      prefix: process.cwd(),
    })
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.message, 'Actual size (1279) of tarball (http://example.com/foo.tgz) did not match the one specified in \'Content-Length\' header (1048576)')
    t.equal(err['code'], 'BAD_TARBALL_SIZE')
    t.equal(err['expectedSize'], 1048576)
    t.equal(err['receivedSize'], tarballSize)
    t.equal(err['attempts'], 2)
    t.end()
  }
})

test('retry when tarball size does not match content-length', async t => {
  const scope = nock(registry)
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

  const unpackTo = path.resolve('unpacked')
  const cachedTarballLocation = path.resolve('cached')
  const resolution = { tarball: 'http://example.com/foo.tgz' }

  await fetch.tarball(resolution, unpackTo, {
    cachedTarballLocation,
    prefix: process.cwd(),
  })

  t.end()
})

test('fail when integrity check fails two times in a row', async t => {
  nock(registry)
    .get('/foo.tgz')
    .times(2)
    .replyWithFile(200, path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const unpackTo = path.resolve('unpacked')
  const cachedTarballLocation = path.resolve('cached')
  const resolution = {
    tarball: 'http://example.com/foo.tgz',
    integrity: tarballIntegrity,
  }

  try {
    await fetch.tarball(resolution, unpackTo, {
      cachedTarballLocation,
      prefix: process.cwd(),
    })
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.message, 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY= integrity checksum failed when using sha1: wanted sha1-HssnaJydJVE+rbyZFKc/VAi+enY= but got sha1-ACjKMFA7S6uRFXSDFfH4aT+4B4Y=. (1194 bytes)')
    t.equal(err['code'], 'EINTEGRITY')
    t.equal(err['resource'], 'http://example.com/foo.tgz')
    t.equal(err['attempts'], 2)
    t.end()
  }
})

test('retry when integrity check fails', async t => {
  nock(registry)
    .get('/foo.tgz')
    .replyWithFile(200, path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  nock(registry)
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const unpackTo = path.resolve('unpacked')
  const cachedTarballLocation = path.resolve('cached')
  const resolution = {
    tarball: 'http://example.com/foo.tgz',
    integrity: tarballIntegrity,
  }

  const params: any = []
  await fetch.tarball(resolution, unpackTo, {
    cachedTarballLocation,
    prefix: process.cwd(),
    onStart (size, attempts) {
      params.push([size, attempts])
    },
  })

  t.deepEqual(params[0], [1194, 1])
  t.deepEqual(params[1], [tarballSize, 2])

  t.end()
})
