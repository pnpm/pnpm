import test = require('tape')
import createResolveFromNpm from '@pnpm/npm-resolver'
import tempy = require('tempy')
import nock = require('nock')
import path = require('path')
import exists = require('path-exists')
import loadJsonFile = require('load-json-file')

const isPositiveMeta = loadJsonFile.sync(path.join(__dirname, 'meta', 'is-positive.json'))
const isPositiveMetaWithDeprecated = loadJsonFile.sync(path.join(__dirname, 'meta', 'is-positive-with-deprecated.json'))
const isPositiveMetaFull = loadJsonFile.sync(path.join(__dirname, 'meta', 'is-positive-full.json'))
const sindresorhusIsMeta = loadJsonFile.sync(path.join(__dirname, 'meta', 'sindresorhus-is.json'))

const registry = 'https://registry.npmjs.org/'

test('resolveFromNpm()', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const store = tempy.directory()
  const resolve = createResolveFromNpm({
    metaCache: new Map(),
    store,
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolve({alias: 'is-positive', pref: '1.0.0'}, {
    registry,
  })

  t.equal(resolveResult!.resolvedVia, 'npm-registry')
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')
  t.equal(resolveResult!.latest!.split('.').length, 3)
  t.deepEqual(resolveResult!.resolution, {
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    registry,
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  t.ok(resolveResult!.package)
  t.ok(resolveResult!.package!.name, 'is-positive')
  t.ok(resolveResult!.package!.version, '1.0.0')

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  setTimeout(async () => {
    const meta = await loadJsonFile(path.join(store, resolveResult!.id, '..', 'index.json'))
    t.ok(meta.name)
    t.ok(meta.versions)
    t.ok(meta['dist-tags'])
    t.end()
  }, 500)
})

test('dry run', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const store = tempy.directory()
  const resolve = createResolveFromNpm({
    metaCache: new Map(),
    store,
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolve({alias: 'is-positive', pref: '1.0.0'}, {
    dryRun: true,
    registry,
  })

  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')
  t.equal(resolveResult!.latest!.split('.').length, 3)
  t.deepEqual(resolveResult!.resolution, {
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    registry,
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  t.ok(resolveResult!.package)
  t.ok(resolveResult!.package!.name, 'is-positive')
  t.ok(resolveResult!.package!.version, '1.0.0')

  // The resolve function does not wait for the package meta cache file to be saved
  // so we must delay for a bit in order to read it
  setTimeout(async () => {
    t.notOk(await exists(path.join(store, resolveResult!.id, '..', 'index.json')))
    t.end()
  }, 500)
})

test('resolve to latest when no pref specified', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'is-positive'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.1.0')
  t.end()
})

test('resolve to defaultTag when no pref specified', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'is-positive'}, {
    defaultTag: 'stable',
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.0.0')
  t.end()
})

test('resolve to biggest non-deprecate version that satisfies the range', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMetaWithDeprecated)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'is-positive', pref: '3'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.0.0')
  t.end()
})

test('can resolve aliased dependency', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'positive', pref: 'npm:is-positive@1.0.0'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')
  t.end()
})

test('can resolve aliased dependency w/o version specifier', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'positive', pref: 'npm:is-positive'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.1.0')
  t.end()
})

test('can resolve aliased dependency w/o version specifier to default tag', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'positive', pref: 'npm:is-positive'}, {
    defaultTag: 'stable',
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.0.0')
  t.end()
})

test('can resolve aliased scoped dependency', async t => {
  nock(registry)
    .get('/@sindresorhus%2Fis')
    .reply(200, sindresorhusIsMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'is', pref: 'npm:@sindresorhus/is@0.6.0'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/@sindresorhus/is/0.6.0')
  t.end()
})

test('can resolve aliased scoped dependency w/o version specifier', async t => {
  nock(registry)
    .get('/@sindresorhus%2Fis')
    .reply(200, sindresorhusIsMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'is', pref: 'npm:@sindresorhus/is'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/@sindresorhus/is/0.7.0')
  t.end()
})

test('can resolve package with version prefixed with v', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'is-positive', pref: 'v1.0.0'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')
  t.end()
})

test('can resolve package version loosely', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({alias: 'is-positive', pref: '= 1.0.0'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')
  t.end()
})

test("resolves to latest if it's inside the wanted range. Even if there are newer versions available inside the range", async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.0.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    registry,
  })

  // 3.1.0 is available but latest is 3.0.0, so preferring it
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.0.0')
  t.end()
})

test("resolves to latest if it's inside the preferred range. Even if there are newer versions available inside the preferred range", async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.0.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    registry,
    preferredVersions: {
      'is-positive': {type: 'range', selector: '^3.0.0'},
    },
  })

  // 3.1.0 is available but latest is 3.0.0, so preferring it
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.0.0')
  t.end()
})

test("resolve using the wanted range, when it doesn't intersect with the preferred range. Even if the preferred range contains the latest version", async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '2.0.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    registry,
    preferredVersions: {
      'is-positive': {type: 'range', selector: '^2.0.0'},
    },
  })

  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.1.0')
  t.end()
})

test("use the preferred version if it's inside the wanted range", async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    registry,
    preferredVersions: {
      'is-positive': {type: 'version', selector: '3.0.0'},
    },
  })

  // 3.1.0 is the latest but we prefer the 3.0.0
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.0.0')
  t.end()
})

test("ignore the preferred version if it's not inside the wanted range", async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    registry,
    preferredVersions: {
      'is-positive': {type: 'version', selector: '2.0.0'},
    },
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.1.0')
  t.end()
})

test('use the preferred range if it intersects with the wanted range', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '1.0.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '>=1.0.0',
  }, {
    registry,
    preferredVersions: {
      'is-positive': {type: 'range', selector: '^3.0.0'},
    },
  })

  // 1.0.0 is the latest but we prefer a version that is also in the preferred range
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.1.0')
  t.end()
})

test("ignore the preferred range if it doesn't intersect with the wanted range", async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    registry,
    preferredVersions: {
      'is-positive': {type: 'range', selector: '^2.0.0'},
    },
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.1.0')
  t.end()
})

test("use the preferred dist-tag if it's inside the wanted range", async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': {
        latest: '3.1.0',
        stable: '3.0.0',
      },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    registry,
    preferredVersions: {
      'is-positive': {type: 'tag', selector: 'stable'},
    },
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.0.0')
  t.end()
})

test("ignore the preferred dist-tag if it's not inside the wanted range", async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': {
        latest: '3.1.0',
        stable: '2.0.0',
      },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    registry,
    preferredVersions: {
      'is-positive': {type: 'tag', selector: 'stable'},
    },
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.1.0')
  t.end()
})

test("prefer a version that is both inside the wanted and preferred ranges. Even if it's not the latest of any of them", async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': {
        latest: '3.0.0',
      },
    })

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '1.0.0 || 2.0.0',
  }, {
    registry,
    preferredVersions: {
      'is-positive': {type: 'range', selector: '1.0.0 || 3.0.0'},
    },
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')
  t.end()
})

test('offline resolution fails when package meta not found in the store', async t => {
  const resolve = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
    offline: true,
  })

  try {
    await resolve({ alias: 'is-positive', pref: '1.0.0' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'NO_OFFLINE_META', 'failed with correct error code')
    t.end()
  }
})

test('offline resolution succeeds when package meta is found in the store', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const store = tempy.directory()

  {
    const resolve = createResolveFromNpm({
      metaCache: new Map(),
      store,
      rawNpmConfig: { registry },
      offline: false,
    })

    // This request will save the package's meta in the store
    await resolve({ alias: 'is-positive', pref: '1.0.0' }, { registry })
  }

  {
    const resolve = createResolveFromNpm({
      metaCache: new Map(),
      store,
      rawNpmConfig: { registry },
      offline: true,
    })

    const resolveResult = await resolve({ alias: 'is-positive', pref: '1.0.0' }, { registry })
    t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')
  }

  t.end()
})

test('prefer offline resolution does not fail when package meta not found in the store', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolve = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
    preferOffline: true,
  })

  const resolveResult = await resolve({ alias: 'is-positive', pref: '1.0.0' }, { registry })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')

  t.end()
})

test('when prefer offline is used, meta from store is used, where latest might be out-of-date', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.0.0' },
    })

  const store = tempy.directory()

  {
    const resolve = createResolveFromNpm({
      metaCache: new Map(),
      store,
      rawNpmConfig: { registry },
    })

    // This request will save the package's meta in the store
    await resolve({ alias: 'is-positive', pref: '1.0.0' }, { registry })
  }

  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  {
    const resolve = createResolveFromNpm({
      metaCache: new Map(),
      store,
      rawNpmConfig: { registry },
      preferOffline: true,
    })

    const resolveResult = await resolve({ alias: 'is-positive', pref: '^3.0.0' }, { registry })
    t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/3.0.0')
  }

  nock.cleanAll()
  t.end()
})

test('error is thrown when package is not found in the registry', async t => {
  const notExistingPackage = 'foo'

  nock(registry)
    .get(`/${notExistingPackage}`)
    .reply(404, {})

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  try {
    await resolveFromNpm({ alias: notExistingPackage, pref: '1.0.0' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, `404 Not Found: ${notExistingPackage}`)
    t.equal(err['package'], notExistingPackage)
    t.equal(err['code'], 'E404')
    t.equal(err['uri'], `${registry}${notExistingPackage}`)
    t.end()
  }
})

test('error is thrown when there is no package found for the requested version', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  try {
    await resolveFromNpm({ alias: 'is-positive', pref: '1000.0.0' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.startsWith('No compatible version found: is-positive@1000.0.0'), 'failed with correct error message')
    t.end()
  }
})

test('error is thrown when package needs authorization', async t => {
  nock(registry)
    .get('/needs-auth')
    .reply(403)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  try {
    await resolveFromNpm({ alias: 'needs-auth', pref: '*' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, '403 Forbidden: needs-auth')
    t.equal(err['package'], 'needs-auth')
    t.equal(err['code'], 'E403')
    t.equal(err['uri'], `${registry}needs-auth`)
    t.end()
  }
})

test('error is thrown when there is no package found for the requested range', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  try {
    await resolveFromNpm({ alias: 'is-positive', pref: '^1000.0.0' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.startsWith('No compatible version found: is-positive@>=1000.0.0 <1001.0.0'), 'failed with correct error message')
    t.end()
  }
})

test('error is thrown when there is no package found for the requested tag', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const resolveFromNpm = createResolveFromNpm({
    metaCache: new Map(),
    store: tempy.directory(),
    rawNpmConfig: { registry },
  })
  try {
    await resolveFromNpm({ alias: 'is-positive', pref: 'unknown-tag' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.startsWith('No compatible version found: is-positive@unknown-tag'), 'failed with correct error message')
    t.end()
  }
})

test('resolveFromNpm() loads full metadata even if non-full metadata is alread cached in store', async t => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)
    .get('/is-positive')
    .reply(200, isPositiveMetaFull)

  const store = tempy.directory()
  t.comment(`store at ${store}`)

  {
    const resolve = createResolveFromNpm({
      fullMetadata: false,
      metaCache: new Map(),
      store,
      rawNpmConfig: { registry },
    })
    const resolveResult = await resolve({alias: 'is-positive', pref: '1.0.0'}, {
      registry,
    })
    t.notOk(resolveResult!.package!['scripts'])
  }

  {
    const resolve = createResolveFromNpm({
      fullMetadata: true,
      metaCache: new Map(),
      store,
      rawNpmConfig: { registry },
    })
    const resolveResult = await resolve({alias: 'is-positive', pref: '1.0.0'}, {
      registry,
    })
    t.ok(resolveResult!.package!['scripts'])
  }

  t.end()
})
