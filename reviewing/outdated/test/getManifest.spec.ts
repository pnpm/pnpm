import { type ResolveFunction } from '@pnpm/client'
import { type PkgResolutionId, type TarballResolution } from '@pnpm/resolver-base'
import { getManifest } from '../lib/createManifestGetter.js'

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

  expect(await getManifest({ ...opts, resolve }, 'foo', 'latest')).toStrictEqual({
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

  expect(await getManifest({ ...opts, resolve: resolve2 }, '@scope/foo', 'latest')).toStrictEqual({
    name: 'foo',
    version: '2.0.0',
  })
})

test('getManifest() with minimumReleaseAge filters latest when too new', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
    minimumReleaseAge: 10080,
  }

  const publishedBy = new Date(Date.now() - 10080 * 60 * 1000)

  const resolve: ResolveFunction = jest.fn(async function (wantedPackage, resolveOpts) {
    expect(wantedPackage.bareSpecifier).toBe('latest')
    expect(resolveOpts.publishedBy).toBeInstanceOf(Date)

    // Simulate latest version being too new
    const error = new Error('No matching version found') as Error & { code?: string }
    error.code = 'ERR_PNPM_NO_MATCHING_VERSION'
    throw error
  })

  const result = await getManifest({ ...opts, resolve, publishedBy }, 'foo', 'latest')

  expect(result).toBeNull()
  expect(resolve).toHaveBeenCalledTimes(1)
})

test('getManifest() does not convert non-latest specifiers', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
  }

  const resolve: ResolveFunction = jest.fn(async function (wantedPackage, resolveOpts) {
    expect(wantedPackage.bareSpecifier).toBe('^1.0.0')

    return {
      id: 'foo/1.5.0' as PkgResolutionId,
      latest: '2.0.0',
      manifest: {
        name: 'foo',
        version: '1.5.0',
      },
      resolution: {} as TarballResolution,
      resolvedVia: 'npm-registry',
    }
  })

  await getManifest({ ...opts, resolve }, 'foo', '^1.0.0')
  expect(resolve).toHaveBeenCalledTimes(1)
})

test('getManifest() handles NO_MATCHING_VERSION error gracefully', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
  }

  const publishedBy = new Date(Date.now() - 10080 * 60 * 1000)

  const resolve: ResolveFunction = jest.fn(async function () {
    const error = new Error('No matching version found') as Error & { code?: string }
    error.code = 'ERR_PNPM_NO_MATCHING_VERSION'
    throw error
  })

  const result = await getManifest({ ...opts, resolve, publishedBy }, 'foo', 'latest')

  // Should return null when no version matches minimumReleaseAge
  expect(result).toBeNull()
})

test('getManifest() with minimumReleaseAgeExclude', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
  }

  const publishedBy = new Date(Date.now() - 10080 * 60 * 1000)
  const isExcludedMatcher = (packageName: string) => packageName === 'excluded-package'

  const resolve: ResolveFunction = jest.fn(async function (wantedPackage, resolveOpts) {
    // Excluded package should not have publishedBy set
    expect(resolveOpts.publishedBy).toBeUndefined()

    return {
      id: 'excluded-package/2.0.0' as PkgResolutionId,
      latest: '2.0.0',
      manifest: {
        name: 'excluded-package',
        version: '2.0.0',
      },
      resolution: {} as TarballResolution,
      resolvedVia: 'npm-registry',
    }
  })

  await getManifest({ ...opts, resolve, isExcludedMatcher, publishedBy }, 'excluded-package', 'latest')
  expect(resolve).toHaveBeenCalledTimes(1)
})
