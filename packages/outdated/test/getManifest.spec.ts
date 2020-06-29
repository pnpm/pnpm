import { ResolveFunction } from '@pnpm/default-resolver'
import test = require('tape')
import { getManifest } from '../lib/createManifestGetter'

test('getManifest()', async (t) => {
  t.plan(4)

  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
    registries: {
      '@scope': 'https://pnpm.js.org/',
      'default': 'https://registry.npmjs.org/',
    },
  }
  {
    const resolve: ResolveFunction = async function (wantedPackage, opts) {
      t.equal(opts.registry, 'https://registry.npmjs.org/')
      return {
        id: 'foo/1.0.0',
        latest: '1.0.0',
        manifest: {
          name: 'foo',
          version: '1.0.0',
        },
        resolution: {
          type: 'tarball',
        },
        resolvedVia: 'npm-registry',
      }
    }
    t.deepEqual(await getManifest(resolve, opts, 'foo', 'latest'), {
      name: 'foo',
      version: '1.0.0',
    })
  }
  {
    const resolve: ResolveFunction = async function (wantedPackage, opts) {
      t.equal(opts.registry, 'https://pnpm.js.org/')
      return {
        id: 'foo/2.0.0',
        latest: '2.0.0',
        manifest: {
          name: 'foo',
          version: '2.0.0',
        },
        resolution: {
          type: 'tarball',
        },
        resolvedVia: 'npm-registry',
      }
    }
    t.deepEqual(await getManifest(resolve, opts, '@scope/foo', 'latest'), {
      name: 'foo',
      version: '2.0.0',
    })
  }
})
