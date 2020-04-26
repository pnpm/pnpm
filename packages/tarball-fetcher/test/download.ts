///<reference path="../../../typings/index.d.ts" />
import createCafs, { getFilePathInCafs as _getFilePathInCafs } from '@pnpm/cafs'
import { LogBase, streamParser } from '@pnpm/logger'
import readPackage from '@pnpm/read-package-json'
import createFetcher from '@pnpm/tarball-fetcher'
import cpFile = require('cp-file')
import { existsSync } from 'fs'
import fs = require('mz/fs')
import nock = require('nock')
import path = require('path')
import ssri = require('ssri')
import test = require('tape')
import tempy = require('tempy')

const cafsDir = tempy.directory()
console.log(cafsDir)
const cafs = createCafs(cafsDir)
const getFilePathInCafs = _getFilePathInCafs.bind(_getFilePathInCafs, cafsDir)

const tarballPath = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-6.24.1.tgz')
const tarballSize = 1279
const tarballIntegrity = 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY='
const registry = 'http://example.com/'
const fetch = createFetcher({
  fetchRetries: 1,
  fetchRetryMaxtimeout: 100,
  fetchRetryMintimeout: 0,
  rawConfig: {
    registry,
  },
  registry,
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

  const cachedTarballLocation = path.resolve('cached')
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
      cachedTarballLocation,
      lockfileDir: process.cwd(),
    })
    t.fail('should have failed')
  } catch (err) {
    t.equal(err.message, 'Actual size (1279) of tarball (http://example.com/foo.tgz) did not match the one specified in \'Content-Length\' header (1048576)')
    t.equal(err['code'], 'ERR_PNPM_BAD_TARBALL_SIZE')
    t.equal(err['expectedSize'], 1048576)
    t.equal(err['receivedSize'], tarballSize)
    t.equal(err['attempts'], 2)

    t.notOk(existsSync(cachedTarballLocation), 'invalid tarball not saved')

    t.ok(scope.isDone())
    t.end()
  }
})

test('redownload the tarball when the one in cache does not satisfy integrity', async t => {
  const scope = nock(registry)
    .get('/babel-helper-hoist-variables-6.24.1.tgz')
    .times(1)
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  const cacheDir = tempy.directory()
  t.comment(`temp dir ${cacheDir}`)

  const cachedTarballLocation = path.join(cacheDir, 'cache.tgz')
  await cpFile(
    path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'),
    cachedTarballLocation,
  )

  const resolution = {
    integrity: tarballIntegrity,
    tarball: `${registry}babel-helper-hoist-variables-6.24.1.tgz`,
  }

  t.plan(3)
  function reporter (log: LogBase & {level: string, name: string, message: string}) {
    if (log.level === 'warn' && log.name === 'pnpm:global' && log.message.startsWith(`The cached tarball at "${cachedTarballLocation}"`)) {
      t.pass('warning logged')
    }
  }
  streamParser.on('data', reporter as any) // tslint:disable-line:no-any
  const { filesIndex } = await fetch.tarball(cafs, resolution, {
    cachedTarballLocation,
    lockfileDir: process.cwd(),
  })
  streamParser.removeListener('data', reporter as any) // tslint:disable-line:no-any

  const pkgJsonIntegrity = await filesIndex['package.json'].generatingIntegrity
  t.equal((await readPackage(getFilePathInCafs({ integrity: pkgJsonIntegrity, ...filesIndex['package.json'] }))).version, '6.24.1')

  t.ok(scope.isDone())
  t.end()
})

test('fail when the tarball in the cache does not pass integrity check in offline mode', async t => {
  const cacheDir = tempy.directory()
  t.comment(`temp dir ${cacheDir}`)

  const cachedTarballLocation = path.join(cacheDir, 'cache.tgz')
  await cpFile(
    path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'),
    cachedTarballLocation,
  )

  const resolution = {
    integrity: tarballIntegrity,
    tarball: `${registry}babel-helper-hoist-variables-6.24.1.tgz`,
  }

  let err!: Error
  try {
    const fetch = createFetcher({
      fetchRetries: 1,
      fetchRetryMaxtimeout: 100,
      fetchRetryMintimeout: 0,
      offline: true,
      rawConfig: {
        registry,
      },
      registry,
    })
    await fetch.tarball(cafs, resolution, {
      cachedTarballLocation,
      lockfileDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  t.equal(err['code'], 'ERR_PNPM_BAD_TARBALL_CHECKSUM')
  t.end()
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

  const cachedTarballLocation = path.resolve('cached.tgz')
  const resolution = { tarball: 'http://example.com/foo.tgz' }

  const result = await fetch.tarball(cafs, resolution, {
    cachedTarballLocation,
    lockfileDir: process.cwd(),
  })

  // fetch.tarball() doesn't wait till the cached tarball is renamed.
  // So this may happen a bit later
  setTimeout(() => {
    t.ok(existsSync(cachedTarballLocation), 'tarball saved') // it is actually not a big issue if the tarball is not there
    t.ok(nock.isDone())
    t.end()
  }, 100)
})

test('redownload incomplete cached tarballs', async t => {
  if (+process.version.split('.')[0].substr(1) >= 10) {
    // TODO: investigate why the following error happens on Node>=10:
    // node[30990]: ../src/node_file.cc:1715:void node::fs::WriteBuffer(const v8::FunctionCallbackInfo<v8::Value>&): Assertion `args[3]->IsInt32()' failed.
    t.skip('This test is skipped on Node.js 10')
    t.end()
    return
  }
  const scope = nock(registry)
    .get('/foo.tgz')
    .replyWithFile(200, tarballPath, {
      'Content-Length': tarballSize.toString(),
    })

  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const cachedTarballLocation = path.resolve('cached')
  const cachedTarballFd = await fs.open(cachedTarballLocation, 'w')
  const tarballData = await fs.readFile(tarballPath)
  await fs.write(cachedTarballFd, tarballData, 0, tarballSize / 2)
  await fs.close(cachedTarballFd)

  const resolution = { tarball: 'http://example.com/foo.tgz' }

  t.plan(2)
  function reporter (log: LogBase & {level: string, name: string, message: string}) {
    if (log.level === 'warn' && log.name === 'pnpm:store' && log.message.startsWith(`Redownloading corrupted cached tarball: ${cachedTarballLocation}`)) {
      t.pass('warning logged')
    }
  }
  streamParser.on('data', reporter as any) // tslint:disable-line:no-any
  try {
    await fetch.tarball(cafs, resolution, {
      cachedTarballLocation,
      lockfileDir: process.cwd(),
    })
  } catch (err) {
    nock.cleanAll()
    t.fail(err)
  }
  streamParser.removeListener('data', reporter as any) // tslint:disable-line:no-any

  t.ok(scope.isDone())
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

  const cachedTarballLocation = path.resolve('cached')
  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  try {
    await fetch.tarball(cafs, resolution, {
      cachedTarballLocation,
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

  const cachedTarballLocation = path.resolve('cached')
  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  const params: Array<[number | null, number]> = []
  await fetch.tarball(cafs, resolution, {
    cachedTarballLocation,
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

  const cachedTarballLocation = path.resolve('cached')
  const tarballAbsoluteLocation = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const tarball = path.relative(process.cwd(), tarballAbsoluteLocation)
  const resolution = {
    integrity: tarballIntegrity,
    tarball: `file:${tarball}`,
  }

  let err: Error | null = null
  try {
    await fetch.tarball(cafs, resolution, {
      cachedTarballLocation,
      lockfileDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }

  t.ok(err, 'error thrown')
  t.equal(err && err.message, 'sha1-HssnaJydJVE+rbyZFKc/VAi+enY= integrity checksum failed when using sha1: ' +
    'wanted sha1-HssnaJydJVE+rbyZFKc/VAi+enY= but got sha512-VuFL1iPaIxJK/k3gTxStIkc6+wSiDwlLdnCWNZyapsVLobu/0onvGOZolASZpfBFiDJYrOIGiDzgLIULTW61Vg== sha1-ACjKMFA7S6uRFXSDFfH4aT+4B4Y=. (1194 bytes)')
  t.equal(err && err['code'], 'EINTEGRITY')
  t.equal(err && err['resource'], tarballAbsoluteLocation)
  t.equal(err && err['attempts'], 1)

  t.end()
})

test("don't fail when integrity check of local file succeeds", async (t) => {
  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const cachedTarballLocation = path.resolve('cached')
  const tarballAbsoluteLocation = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const tarball = path.relative(process.cwd(), tarballAbsoluteLocation)
  const resolution = {
    integrity: await getFileIntegrity(tarballAbsoluteLocation),
    tarball: `file:${tarball}`,
  }

  const { filesIndex } = await fetch.tarball(cafs, resolution, {
    cachedTarballLocation,
    lockfileDir: process.cwd(),
  })

  t.equal(typeof filesIndex['package.json'], 'object', 'files index returned')

  t.end()
})

test("don't fail when the cache tarball does not exist", async (t) => {
  nock(registry)
    .get('/foo.tgz')
    .times(1)
    .replyWithFile(200, path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz'), {
      'Content-Length': '1194',
    })

  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const cachedTarballLocation = path.resolve('dir', 'cached')
  const tarballAbsoluteLocation = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const resolution = {
    integrity: await getFileIntegrity(tarballAbsoluteLocation),
    tarball: `${registry}foo.tgz`,
  }

  const { filesIndex } = await fetch.tarball(cafs, resolution, {
    cachedTarballLocation,
    lockfileDir: process.cwd(),
  })

  t.equal(typeof filesIndex['package.json'], 'object', 'files index returned')

  t.end()
})

test('fail when the cache tarball does not exist in offline mode', async (t) => {
  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const cachedTarballLocation = path.resolve('dir', 'cached')
  const tarballAbsoluteLocation = path.join(__dirname, 'tars', 'babel-helper-hoist-variables-7.0.0-alpha.10.tgz')
  const resolution = {
    integrity: await getFileIntegrity(tarballAbsoluteLocation),
    tarball: `${registry}foo.tgz`,
  }

  let err!: Error
  try {
    const fetch = createFetcher({
      fetchRetries: 1,
      fetchRetryMaxtimeout: 100,
      fetchRetryMintimeout: 0,
      offline: true,
      rawConfig: {
        registry,
      },
      registry,
    })
    await fetch.tarball(cafs, resolution, {
      cachedTarballLocation,
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

  const cachedTarballLocation = path.resolve('cached')
  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  const index = await fetch.tarball(cafs, resolution, {
    cachedTarballLocation,
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

  const cachedTarballLocation = path.resolve('cached')
  const resolution = {
    integrity: tarballIntegrity,
    tarball: 'http://example.com/foo.tgz',
  }

  let err!: Error

  try {
    await fetch.tarball(cafs, resolution, {
      cachedTarballLocation,
      lockfileDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  err = err || new Error()
  t.equal(err.message, '403 Forbidden: http://example.com/foo.tgz')
  t.equal(err['code'], 'ERR_PNPM_TARBALL_FETCH')
  t.equal(err['httpStatusCode'], 403)
  t.equal(err['uri'], 'http://example.com/foo.tgz')

  t.ok(scope.isDone())
  t.end()
})

test('accessing private packages', async t => {
  const scope = nock(
    registry,
    {
      reqheaders: {
        'authorization': 'Bearer ofjergrg349gj3f2',
      },
    },
  )
  .get('/foo.tgz')
  .replyWithFile(200, tarballPath, {
    'Content-Length': tarballSize.toString(),
  })

  process.chdir(tempy.directory())
  t.comment(`testing in ${process.cwd()}`)

  const fetch = createFetcher({
    alwaysAuth: true,
    fetchRetries: 1,
    fetchRetryMaxtimeout: 100,
    fetchRetryMintimeout: 0,
    rawConfig: {
      '//example.com/:_authToken': 'ofjergrg349gj3f2',
      registry,
    },
    registry,
  })

  const cachedTarballLocation = path.resolve('cached')
  const resolution = {
    integrity: tarballIntegrity,
    registry,
    tarball: 'http://example.com/foo.tgz',
  }

  const index = await fetch.tarball(cafs, resolution, {
    cachedTarballLocation,
    lockfileDir: process.cwd(),
  })

  t.ok(index)

  t.ok(scope.isDone())
  t.end()
})

async function getFileIntegrity (filename: string) {
  return (await ssri.fromStream(fs.createReadStream(filename))).toString()
}
