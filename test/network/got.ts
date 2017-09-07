import test = require('tape')
import nock = require('nock')
import createGot, {NpmRegistryClient} from '../../src/network/got'
import path = require('path')
import RegClient = require('npm-registry-client')

const tmpDir = path.join(__dirname, '..', '..', '.tmp')

test('fail when tarball size does not match content-length and no retry passed', async t => {
  nock('http://example.com')
    .get('/foo.tgz')
    .replyWithFile(200, path.join(__dirname, '..', 'tars', 'babel-helper-hoist-variables-6.24.1.tgz'), {
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
    t.equal(err['receivedSize'], 1279)
    t.end()
  }
})
