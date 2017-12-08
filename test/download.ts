import test = require('tape')
import nock = require('nock')
import createDownloader, {NpmRegistryClient} from '@pnpm/tarball-fetcher/lib/createDownloader'
import path = require('path')
import tempy = require('tempy')

const tarballPath = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-6.24.1.tgz')
const tarballSize = 1279
const tarballIntegrity = 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY='
const RETRY = {
  retries: 1,
  minTimeout: 0,
  maxTimeout: 100,
}

test('fail when tarball size does not match content-length', async t => {
  nock('http://example.com')
    .get('/foo.tgz')
    .times(2)
    .replyWithFile(200, tarballPath, {
      'Content-Length': (1024 * 1024).toString(),
    })

  const download = createDownloader({
    alwaysAuth: false,
    registry: 'http://example.com/',
    retry: RETRY,
  })

  try {
    const tmpDir = tempy.directory()
    await download('http://example.com/foo.tgz', path.join(tmpDir, 'foo.tgz'), {
      unpackTo: path.join(tmpDir, 'unpacked'),
      generatePackageIntegrity: false,
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
  const scope = nock('http://example.com')
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': (1024 * 1024).toString(),
    })

  nock('http://example.com')
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  const download = createDownloader({
    alwaysAuth: false,
    registry: 'http://example.com/',
    retry: RETRY,
  })

  const tmpDir = tempy.directory()
  await download('http://example.com/foo.tgz', path.join(tmpDir, 'foo.tgz'), {
    unpackTo: path.join(tmpDir, 'unpacked'),
    generatePackageIntegrity: false,
  })
  t.end()
})

test('fail when integrity check fails two times in a row', async t => {
  nock('http://example.com')
    .get('/foo.tgz')
    .times(2)
    .replyWithFile(200, path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  const download = createDownloader({
    alwaysAuth: false,
    registry: 'http://example.com/',
    retry: RETRY,
  })

  try {
    const tmpDir = tempy.directory()
    await download('http://example.com/foo.tgz', path.join(tmpDir, 'foo.tgz'), {
      unpackTo: path.join(tmpDir, 'unpacked'),
      generatePackageIntegrity: false,
      integrity: tarballIntegrity,
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
  nock('http://example.com')
    .get('/foo.tgz')
    .replyWithFile(200, path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  nock('http://example.com')
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  const download = createDownloader({
    alwaysAuth: false,
    registry: 'http://example.com/',
    retry: RETRY,
  })

  const params: any = []
  const tmpDir = tempy.directory()
  await download('http://example.com/foo.tgz', path.join(tmpDir, 'foo.tgz'), {
    unpackTo: path.join(tmpDir, 'unpacked'),
    generatePackageIntegrity: false,
    integrity: tarballIntegrity,
    onStart (size, attempts) {
      params.push([size, attempts])
    },
  })

  t.deepEqual(params[0], [1194, 1])
  t.deepEqual(params[1], [tarballSize, 2])

  t.end()
})
