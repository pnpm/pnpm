import { ResolveFunction } from '@pnpm/client'
import { getManifest } from '../lib/createManifestGetter'

test('getManifest()', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
    registries: {
      '@scope': 'https://pnpm.js.org/',
      default: 'https://registry.npmjs.org/',
    },
  }

  const resolve: ResolveFunction = async function (wantedPackage, opts) {
    expect(opts.registry).toEqual('https://registry.npmjs.org/')
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

  expect(await getManifest(resolve, opts, 'foo', 'latest')).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
  })

  const resolve2: ResolveFunction = async function (wantedPackage, opts) {
    expect(opts.registry).toEqual('https://pnpm.js.org/')
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

  expect(await getManifest(resolve2, opts, '@scope/foo', 'latest')).toStrictEqual({
    name: 'foo',
    version: '2.0.0',
  })
})
