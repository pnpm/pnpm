import { pkgSnapshotToResolutionWithResolvers } from '../lib/pkgSnapshotToResolution.js'
import { type PackageSnapshot } from '@pnpm/lockfile.types'

test('throws error for unsupported custom resolution type', async () => {
  const pkgSnapshot: PackageSnapshot = {
    resolution: {
      type: 'custom-type',
      customField: 'value',
    },
  }

  try {
    await pkgSnapshotToResolutionWithResolvers(
      'pkg@1.0.0',
      pkgSnapshot,
      { default: 'https://registry.npmjs.org/' },
      {
        lockfileDir: '/test',
        projectDir: '/test',
      }
    )
    fail('Should have thrown an error')
  } catch (err: unknown) {
    expect((err as { code: string }).code).toBe('ERR_PNPM_UNSUPPORTED_LOCKFILE_RESOLUTION')
    expect((err as { message: string }).message).toContain('custom-type')
    expect((err as { message: string }).message).toContain('No custom resolver plugin is available')
  }
})

test('custom resolver handles custom resolution type', async () => {
  const pkgSnapshot: PackageSnapshot = {
    resolution: {
      type: 'brazil',
      hash: 'abc123',
      integrity: 'sha512-test',
    },
  }

  const customResolver = {
    supportsLockfileResolution: (_pkgId: string, resolution: unknown) => {
      return (resolution as { type?: string }).type === 'brazil'
    },
    fromLockfileResolution: (_pkgId: string, resolution: unknown) => {
      const brazilRes = resolution as { hash: string, integrity: string }
      return {
        tarball: `file:/path/to/${brazilRes.hash}.tgz`,
        integrity: brazilRes.integrity,
      }
    },
  }

  const result = await pkgSnapshotToResolutionWithResolvers(
    'pkg@brazil:abc123',
    pkgSnapshot,
    { default: 'https://registry.npmjs.org/' },
    {
      customResolvers: [customResolver],
      lockfileDir: '/test',
      projectDir: '/test',
    }
  )

  expect(result).toEqual({
    tarball: 'file:/path/to/abc123.tgz',
    integrity: 'sha512-test',
  })
})

test('custom resolver with async methods handles custom resolution type', async () => {
  const pkgSnapshot: PackageSnapshot = {
    resolution: {
      type: 'async-custom',
      data: 'test-data',
    },
  }

  const customResolver = {
    supportsLockfileResolution: async (_pkgId: string, resolution: unknown) => {
      // Simulate async check (e.g., checking cache or remote source)
      await new Promise(resolve => setTimeout(resolve, 1))
      return (resolution as { type?: string }).type === 'async-custom'
    },
    fromLockfileResolution: async (_pkgId: string, resolution: unknown) => {
      // Simulate async resolution (e.g., fetching from remote source)
      await new Promise(resolve => setTimeout(resolve, 1))
      const customRes = resolution as { data: string }
      return {
        tarball: `file://async-${customRes.data}.tgz`,
        integrity: 'sha512-async',
      }
    },
  }

  const result = await pkgSnapshotToResolutionWithResolvers(
    'pkg@async:test',
    pkgSnapshot,
    { default: 'https://registry.npmjs.org/' },
    {
      customResolvers: [customResolver],
      lockfileDir: '/test',
      projectDir: '/test',
    }
  )

  expect(result).toEqual({
    tarball: 'file://async-test-data.tgz',
    integrity: 'sha512-async',
  })
})

test('multiple custom resolvers - first matching wins for lockfile resolution', async () => {
  const pkgSnapshot: PackageSnapshot = {
    resolution: {
      type: 'shared-custom',
      value: '123',
    },
  }

  const resolver1 = {
    supportsLockfileResolution: (_pkgId: string, resolution: unknown) => {
      return (resolution as { type?: string }).type === 'shared-custom'
    },
    fromLockfileResolution: () => ({
      tarball: 'file://resolver1.tgz',
      integrity: 'sha512-resolver1',
    }),
  }

  const resolver2 = {
    supportsLockfileResolution: (_pkgId: string, resolution: unknown) => {
      return (resolution as { type?: string }).type === 'shared-custom'
    },
    fromLockfileResolution: () => ({
      tarball: 'file://resolver2.tgz',
      integrity: 'sha512-resolver2',
    }),
  }

  const result = await pkgSnapshotToResolutionWithResolvers(
    'pkg@shared:123',
    pkgSnapshot,
    { default: 'https://registry.npmjs.org/' },
    {
      customResolvers: [resolver1, resolver2],
      lockfileDir: '/test',
      projectDir: '/test',
    }
  )

  // First resolver should win
  expect(result).toEqual({
    tarball: 'file://resolver1.tgz',
    integrity: 'sha512-resolver1',
  })
})

test('custom resolver can intercept standard resolution types', async () => {
  const pkgSnapshot: PackageSnapshot = {
    resolution: {
      tarball: 'https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz',
      integrity: 'sha512-original',
    },
  }

  const customResolver = {
    supportsLockfileResolution: (_pkgId: string, resolution: unknown) => {
      // Intercept all tarball resolutions
      return 'tarball' in (resolution as object)
    },
    fromLockfileResolution: (_pkgId: string, resolution: unknown) => {
      // Redirect to local cache
      const res = resolution as { integrity: string }
      return {
        tarball: 'file://local-cache/pkg-1.0.0.tgz',
        integrity: res.integrity,
      }
    },
  }

  const result = await pkgSnapshotToResolutionWithResolvers(
    'pkg@1.0.0',
    pkgSnapshot,
    { default: 'https://registry.npmjs.org/' },
    {
      customResolvers: [customResolver],
      lockfileDir: '/test',
      projectDir: '/test',
    }
  )

  expect(result).toEqual({
    tarball: 'file://local-cache/pkg-1.0.0.tgz',
    integrity: 'sha512-original',
  })
})

test('does not throw for standard resolution types', async () => {
  const pkgSnapshot: PackageSnapshot = {
    resolution: {
      type: 'directory',
      directory: '/path/to/pkg',
    },
  }

  const result = await pkgSnapshotToResolutionWithResolvers(
    'pkg@1.0.0',
    pkgSnapshot,
    { default: 'https://registry.npmjs.org/' },
    {
      lockfileDir: '/test',
      projectDir: '/test',
    }
  )

  expect(result).toEqual({
    type: 'directory',
    directory: '/path/to/pkg',
  })
})
