import { type ResolveFunction } from '@pnpm/client'
import { type PkgResolutionId, type TarballResolution } from '@pnpm/resolver-base'
import { getManifest } from '../lib/createManifestGetter'

test('getManifest()', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
  }

  const resolve: ResolveFunction = async function (wantedPackage, opts) {
    return {
      id: 'foo/1.0.0' as PkgResolutionId,
      latest: '1.0.0',
      manifest: {
        name: 'foo',
        version: '1.0.0',
      },
      resolution: {} as TarballResolution,
      resolvedVia: 'npm-registry',
    }
  }

  expect(await getManifest(resolve, opts, 'foo', 'latest')).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
  })

  const resolve2: ResolveFunction = async function (wantedPackage, opts) {
    return {
      id: 'foo/2.0.0' as PkgResolutionId,
      latest: '2.0.0',
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
      resolution: {} as TarballResolution,
      resolvedVia: 'npm-registry',
    }
  }

  expect(await getManifest(resolve2, opts, '@scope/foo', 'latest')).toStrictEqual({
    name: 'foo',
    version: '2.0.0',
  })
})
