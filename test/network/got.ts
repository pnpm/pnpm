import test = require('tape')
import nock = require('nock')
import createGot, {NpmRegistryClient} from '../../src/network/got'
import path = require('path')
import RegClient = require('npm-registry-client')

const tmpDir = path.join(__dirname, '..', '..', '.tmp')
const tarballPath = path.join(__dirname, '..', 'tars', 'babel-helper-hoist-variables-6.24.1.tgz')
const tarballSize = 1279
const tarballIntegrity = 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY='

test('fail when tarball size does not match content-length and no retry passed', async t => {
  nock('http://example.com')
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': (1024 * 1024).toString(),
    })

  const client: NpmRegistryClient = new RegClient()
  const got = createGot(client, {
    networkConcurrency: 1,
    alwaysAuth: false,
    registry: 'http://example.com/',
    rawNpmConfig: {},
    retries: 0,
  })

  try {
    await got.download('http://example.com/foo.tgz', path.join(tmpDir, 'foo.tgz'), {
      unpackTo: path.join(tmpDir, 'unpacked'),
      generatePackageIntegrity: false,
    })
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.message, 'Actual size (1279) of tarball (http://example.com/foo.tgz) did not match the one specified in \'Content-Length\' header (1048576)')
    t.equal(err['code'], 'BAD_TARBALL_SIZE')
    t.equal(err['expectedSize'], 1048576)
    t.equal(err['receivedSize'], tarballSize)
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

  const client: NpmRegistryClient = new RegClient()
  const got = createGot(client, {
    networkConcurrency: 1,
    alwaysAuth: false,
    registry: 'http://example.com/',
    rawNpmConfig: {},
    retries: 1,
  })

  await got.download('http://example.com/foo.tgz', path.join(tmpDir, 'foo.tgz'), {
    unpackTo: path.join(tmpDir, 'unpacked'),
    generatePackageIntegrity: false,
  })
  t.end()
})

test('fail when integrity check fails and no retry passed', async t => {
  nock('http://example.com')
    .get('/foo.tgz')
    .replyWithFile(200, path.join(__dirname, '..', 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  const client: NpmRegistryClient = new RegClient()
  const got = createGot(client, {
    networkConcurrency: 1,
    alwaysAuth: false,
    registry: 'http://example.com/',
    rawNpmConfig: {},
    retries: 0,
  })

  try {
    await got.download('http://example.com/foo.tgz', path.join(tmpDir, 'foo.tgz'), {
      unpackTo: path.join(tmpDir, 'unpacked'),
      generatePackageIntegrity: false,
      integrity: tarballIntegrity,
    })
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.message, 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY= integrity checksum failed when using sha1: wanted sha1-HssnaJydJVE+rbyZFKc/VAi+enY= but got sha1-ACjKMFA7S6uRFXSDFfH4aT+4B4Y=. (1194 bytes)')
    t.end()
  }
})

test('retry when integrity check fails', async t => {
  nock('http://example.com')
    .get('/foo.tgz')
    .replyWithFile(200, path.join(__dirname, '..', 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  nock('http://example.com')
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  const client: NpmRegistryClient = new RegClient()
  const got = createGot(client, {
    networkConcurrency: 1,
    alwaysAuth: false,
    registry: 'http://example.com/',
    rawNpmConfig: {},
    retries: 1,
  })

  await got.download('http://example.com/foo.tgz', path.join(tmpDir, 'foo.tgz'), {
    unpackTo: path.join(tmpDir, 'unpacked'),
    generatePackageIntegrity: false,
    integrity: tarballIntegrity,
  })
  t.end()
})
