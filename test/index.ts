import test = require('tape')
import resolveFromNpm from '@pnpm/npm-resolver'
import got = require('got')
import tempy = require('tempy')

function getJson (url: string, registry: string) {
  return got(url, {json: true})
    .then((response: any) => response.body)
}

test('resolveFromNpm()', async t => {
  const resolveResult = await resolveFromNpm({alias: 'is-positive', pref: '1.0.0'}, {
    storePath: tempy.directory(),
    registry: 'https://registry.npmjs.org/',
    metaCache: new Map(),
    offline: false,
    getJson,
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
