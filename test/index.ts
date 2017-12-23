import test = require('tape')
import createResolveFromNpm from '@pnpm/npm-resolver'
import tempy = require('tempy')

const resolveFromNpm = createResolveFromNpm({
  metaCache: new Map(),
  store: tempy.directory(),
  rawNpmConfig: {
    registry: 'https://registry.npmjs.org/',
  },
})

test('resolveFromNpm()', async t => {
  const resolveResult = await resolveFromNpm({alias: 'is-positive', pref: '1.0.0'}, {
    registry: 'https://registry.npmjs.org/',
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')
  t.equal(resolveResult!.latest!.split('.').length, 3)
  t.deepEqual(resolveResult!.resolution, {
    integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  t.ok(resolveResult!.package)
  t.ok(resolveResult!.package!.name, 'is-positive')
  t.ok(resolveResult!.package!.version, '1.0.0')
  t.end()
})

test('can resolve aliased dependency', async t => {
  const resolveResult = await resolveFromNpm({alias: 'positive', pref: 'npm:is-positive@1.0.0'}, {
    registry: 'https://registry.npmjs.org/',
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/is-positive/1.0.0')
  t.end()
})

test('can resolve aliased scoped dependency', async t => {
  const resolveResult = await resolveFromNpm({alias: 'is', pref: 'npm:@sindresorhus/is@0.6.0'}, {
    registry: 'https://registry.npmjs.org/',
  })
  t.equal(resolveResult!.id, 'registry.npmjs.org/@sindresorhus/is/0.6.0')
  t.end()
})
