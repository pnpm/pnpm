import { jest } from '@jest/globals'
import { type ResolveFunction } from '@pnpm/client'
import { type PkgResolutionId, type TarballResolution } from '@pnpm/resolver-base'
import { getManifest } from '../lib/createManifestGetter.js'

test('getManifest()', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
  }

  const resolve: ResolveFunction = async function (_wantedPackage, _opts) {
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

  const resolve2: ResolveFunction = async function (_wantedPackage, _opts) {
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

  const resolve = jest.fn<ResolveFunction>(async (wantedPackage, resolveOpts) => {
    expect(wantedPackage.bareSpecifier).toBe('latest')
    expect(resolveOpts.publishedBy).toBeInstanceOf(Date)

    // Simulate latest version being too new
    const error = new Error('No matching version found') as Error & { code?: string }
    error.code = 'ERR_PNPM_NO_MATURE_MATCHING_VERSION'
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

  const resolve = jest.fn<ResolveFunction>(async (wantedPackage) => {
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
    error.code = 'ERR_PNPM_NO_MATURE_MATCHING_VERSION'
    throw error
  })

  const result = await getManifest({ ...opts, resolve, publishedBy }, 'foo', 'latest')

  // Should return null when no version matches minimumReleaseAge
  expect(result).toBeNull()
})

// https://github.com/pnpm/pnpm/issues/10605
test('getManifest() returns null for NO_MATCHING_VERSION when publishedBy is set', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
  }

  const publishedBy = new Date(Date.now() - 10080 * 60 * 1000)

  const resolve: ResolveFunction = jest.fn(async function () {
    // When all versions of a package are newer than minimumReleaseAge and the
    // latest dist-tag points to a pre-release, the resolver may throw
    // NO_MATCHING_VERSION instead of NO_MATURE_MATCHING_VERSION.
    const error = new Error('No matching version found') as Error & { code?: string }
    error.code = 'ERR_PNPM_NO_MATCHING_VERSION'
    throw error
  })

  const result = await getManifest({ ...opts, resolve, publishedBy }, 'foo', 'latest')

  expect(result).toBeNull()
})

// When publishedBy is NOT set, NO_MATCHING_VERSION should still throw.
test('getManifest() throws NO_MATCHING_VERSION when publishedBy is not set', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
  }

  const resolve: ResolveFunction = jest.fn(async function () {
    const error = new Error('No matching version found') as Error & { code?: string }
    error.code = 'ERR_PNPM_NO_MATCHING_VERSION'
    throw error
  })

  await expect(getManifest({ ...opts, resolve }, 'foo', 'latest')).rejects.toThrow('No matching version found')
})

test('getManifest() with minimumReleaseAgeExclude', async () => {
  const opts = {
    dir: '',
    lockfileDir: '',
    rawConfig: {},
  }

  const publishedBy = new Date(Date.now() - 10080 * 60 * 1000)
  const publishedByExclude = (packageName: string) => packageName === 'excluded-package'

  const resolve = jest.fn<ResolveFunction>(async (_wantedPackage, _resolveOpts) => {
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

  await getManifest({ ...opts, resolve, publishedByExclude, publishedBy }, 'excluded-package', 'latest')
  expect(resolve).toHaveBeenCalledTimes(1)
})
