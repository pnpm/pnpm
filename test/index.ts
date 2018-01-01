import test = require('tape')
import createResolveFromNpm from '@pnpm/npm-resolver'
import tempy = require('tempy')
import {addDistTag} from 'pnpm-registry-mock'
import path = require('path')
import loadJsonFile = require('load-json-file')

const registry = 'http://localhost:4873/'
const metaCache = new Map()

const resolveFromNpm = createResolveFromNpm({
  metaCache,
  store: tempy.directory(),
  rawNpmConfig: { registry },
})

test('waiting for verdaccio to startup', t => setTimeout(() => t.end(), 1000))

test('resolveFromNpm()', async t => {
  const store = tempy.directory()
  const resolve = createResolveFromNpm({
    metaCache: new Map(),
    store,
    rawNpmConfig: { registry },
  })
  const resolveResult = await resolve({alias: 'is-positive', pref: '1.0.0'}, {
    registry,
  })

  t.equal(resolveResult!.id, 'localhost+4873/is-positive/1.0.0')
  t.equal(resolveResult!.latest!.split('.').length, 3)
  t.deepEqual(resolveResult!.resolution, {
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
    registry,
    tarball: 'http://localhost:4873/is-positive/-/is-positive-1.0.0.tgz',
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
  }, 100)
})

test('can resolve aliased dependency', async t => {
  metaCache.clear()
  const resolveResult = await resolveFromNpm({alias: 'positive', pref: 'npm:is-positive@1.0.0'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'localhost+4873/is-positive/1.0.0')
  t.end()
})

test('can resolve aliased scoped dependency', async t => {
  metaCache.clear()
  const resolveResult = await resolveFromNpm({alias: 'is', pref: 'npm:@sindresorhus/is@0.6.0'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'localhost+4873/@sindresorhus/is/0.6.0')
  t.end()
})

test('can resolve package with version prefixed with v', async t => {
  metaCache.clear()
  const resolveResult = await resolveFromNpm({alias: 'is-positive', pref: 'v1.0.0'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'localhost+4873/is-positive/1.0.0')
  t.end()
})

test('can resolve package version loosely', async t => {
  metaCache.clear()
  const resolveResult = await resolveFromNpm({alias: 'is-positive', pref: '= 1.0.0'}, {
    registry,
  })
  t.equal(resolveResult!.id, 'localhost+4873/is-positive/1.0.0')
  t.end()
})

test("resolves to latest if it's inside the wanted range. Even if there are newer versions available inside the range", async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '1.0.0', distTag: 'latest'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '^1.0.0',
  }, {
    registry,
  })

  // 1.1.0 is available but latest is 1.0.0, so preferring it
  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.0.0')
  t.end()
})

test("resolves to latest if it's inside the preferred range. Even if there are newer versions available inside the preferred range", async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '1.0.0', distTag: 'latest'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '^1.0.0',
  }, {
    registry,
    preferredVersions: {
      'pnpm-foo': {type: 'range', selector: '^1.0.0'},
    },
  })

  // 1.1.0 is available but latest is 1.0.0, so preferring it
  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.0.0')
  t.end()
})

test("resolve using the wanted range, when it doesn't intersect with the preferred range. Even if the preferred range contains the latest version", async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '2.0.0', distTag: 'latest'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '^1.0.0',
  }, {
    registry,
    preferredVersions: {
      'pnpm-foo': {type: 'range', selector: '^2.0.0'},
    },
  })

  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.3.0')
  t.end()
})

test("use the preferred version if it's inside the wanted range", async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '1.1.0', distTag: 'latest'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '^1.0.0',
  }, {
    registry,
    preferredVersions: {
      'pnpm-foo': {type: 'version', selector: '1.0.0'},
    },
  })

  // 1.1.0 is the latest but we prefer the 1.0.0
  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.0.0')
  t.end()
})

test("ignore the preferred version if it's not inside the wanted range", async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '1.1.0', distTag: 'latest'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '^1.0.0',
  }, {
    registry,
    preferredVersions: {
      'pnpm-foo': {type: 'version', selector: '2.0.0'},
    },
  })
  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.1.0')
  t.end()
})

test('use the preferred range if it intersects with the wanted range', async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '1.0.0', distTag: 'latest'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '^1.0.0',
  }, {
    registry,
    preferredVersions: {
      'pnpm-foo': {type: 'range', selector: '^1.1.0'},
    },
  })

  // 1.0.0 is the latest but we prefer a version that is also in the preferred range
  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.3.0')
  t.end()
})

test("ignore the preferred range if it doesn't intersect with the wanted range", async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '1.1.0', distTag: 'latest'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '^1.0.0',
  }, {
    registry,
    preferredVersions: {
      'pnpm-foo': {type: 'range', selector: '^2.0.0'},
    },
  })
  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.1.0')
  t.end()
})

test("use the preferred dist-tag if it's inside the wanted range", async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '1.1.0', distTag: 'latest'})
  await addDistTag({package: 'pnpm-foo', version: '1.0.0', distTag: 'stable'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '^1.0.0',
  }, {
    registry,
    preferredVersions: {
      'pnpm-foo': {type: 'tag', selector: 'stable'},
    },
  })
  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.0.0')
  t.end()
})

test("ignore the preferred dist-tag if it's not inside the wanted range", async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '1.1.0', distTag: 'latest'})
  await addDistTag({package: 'pnpm-foo', version: '2.0.0', distTag: 'stable'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '^1.0.0',
  }, {
    registry,
    preferredVersions: {
      'pnpm-foo': {type: 'tag', selector: 'stable'},
    },
  })
  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.1.0')
  t.end()
})

test("prefer a version that is both inside the wanted and preferred ranges. Even if it's not the latest of any of them", async t => {
  metaCache.clear()
  await addDistTag({package: 'pnpm-foo', version: '1.2.0', distTag: 'latest'})

  const resolveResult = await resolveFromNpm({
    alias: 'pnpm-foo',
    pref: '1.1.0 || 1.3.0',
  }, {
    registry,
    preferredVersions: {
      'pnpm-foo': {type: 'range', selector: '1.1.0 || 1.2.0'},
    },
  })
  t.equal(resolveResult!.id, 'localhost+4873/pnpm-foo/1.1.0')
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
    t.equal(resolveResult!.id, 'localhost+4873/is-positive/1.0.0')
  }

  t.end()
})

test('error is thrown when package is not found in the registry', async t => {
  try {
    const notExistingPackage = 'sndof240jg34g-kwesdgk'
    await resolveFromNpm({ alias: notExistingPackage, pref: '1.0.0' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, '404 Not Found: sndof240jg34g-kwesdgk')
    t.equal(err['package'], 'sndof240jg34g-kwesdgk')
    t.equal(err['code'], 'E404')
    t.equal(err['uri'], 'http://localhost:4873/sndof240jg34g-kwesdgk')
    t.end()
  }
})

test('error is thrown when there is no package found for the requested version', async t => {
  try {
    await resolveFromNpm({ alias: 'is-positive', pref: '1000.0.0' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.startsWith('No compatible version found: is-positive@1000.0.0'), 'failed with correct error message')
    t.end()
  }
})

test('error is thrown when package needs authorization', async t => {
  try {
    await resolveFromNpm({ alias: 'needs-auth', pref: '*' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, '403 Forbidden: needs-auth')
    t.equal(err['package'], 'needs-auth')
    t.equal(err['code'], 'E403')
    t.equal(err['uri'], 'http://localhost:4873/needs-auth')
    t.end()
  }
})

test('error is thrown when there is no package found for the requested range', async t => {
  try {
    await resolveFromNpm({ alias: 'is-positive', pref: '^1000.0.0' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.startsWith('No compatible version found: is-positive@>=1000.0.0 <1001.0.0'), 'failed with correct error message')
    t.end()
  }
})

test('error is thrown when there is no package found for the requested tag', async t => {
  try {
    await resolveFromNpm({ alias: 'is-positive', pref: 'unknown-tag' }, { registry })
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.startsWith('No compatible version found: is-positive@unknown-tag'), 'failed with correct error message')
    t.end()
  }
})
