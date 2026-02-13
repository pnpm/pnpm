import { checkCustomResolverForceResolve } from '../../src/install/checkCustomResolverForceResolve.js'
import { type CustomResolver } from '@pnpm/hooks.types'
import { type LockfileObject } from '@pnpm/lockfile.types'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyPackages = any

function lockfileWithPackages (packages?: Record<string, object>): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {},
    packages: packages as AnyPackages,
  }
}

const TEST_PKG_SNAPSHOT = {
  resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
}

describe('checkCustomResolverForceResolve', () => {
  test('returns false when no custom resolvers provided', async () => {
    const result = await checkCustomResolverForceResolve([], lockfileWithPackages())

    expect(result).toBe(false)
  })

  test('returns false when lockfile has no packages', async () => {
    const resolver: CustomResolver = {
      shouldRefreshResolution: () => true,
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfileWithPackages())

    expect(result).toBe(false)
  })

  test('returns false when custom resolver has no shouldRefreshResolution', async () => {
    const resolver: CustomResolver = {
      canResolve: () => true,
    }

    const result = await checkCustomResolverForceResolve(
      [resolver],
      lockfileWithPackages({ 'test-pkg@1.0.0': TEST_PKG_SNAPSHOT })
    )

    expect(result).toBe(false)
  })

  test('returns false when shouldRefreshResolution returns false', async () => {
    const resolver: CustomResolver = {
      shouldRefreshResolution: () => false,
    }

    const result = await checkCustomResolverForceResolve(
      [resolver],
      lockfileWithPackages({ 'test-pkg@1.0.0': TEST_PKG_SNAPSHOT })
    )

    expect(result).toBe(false)
  })

  test('returns true when shouldRefreshResolution returns true', async () => {
    const resolver: CustomResolver = {
      shouldRefreshResolution: () => true,
    }

    const result = await checkCustomResolverForceResolve(
      [resolver],
      lockfileWithPackages({ 'test-pkg@1.0.0': TEST_PKG_SNAPSHOT })
    )

    expect(result).toBe(true)
  })

  test('shouldRefreshResolution is called independently of canResolve', async () => {
    // canResolve returning false should NOT prevent shouldRefreshResolution from
    // being called -- they operate on different paths.
    const resolver: CustomResolver = {
      canResolve: () => false,
      shouldRefreshResolution: () => true,
    }

    const result = await checkCustomResolverForceResolve(
      [resolver],
      lockfileWithPackages({ 'test-pkg@1.0.0': TEST_PKG_SNAPSHOT })
    )

    expect(result).toBe(true)
  })

  test('returns true when any resolver among multiple returns true', async () => {
    const resolver1: CustomResolver = {
      shouldRefreshResolution: () => false,
    }
    const resolver2: CustomResolver = {
      shouldRefreshResolution: () => true,
    }

    const result = await checkCustomResolverForceResolve(
      [resolver1, resolver2],
      lockfileWithPackages({ 'test-pkg@1.0.0': TEST_PKG_SNAPSHOT })
    )

    expect(result).toBe(true)
  })

  test('handles async shouldRefreshResolution', async () => {
    const resolver: CustomResolver = {
      shouldRefreshResolution: async () => {
        await new Promise(resolve => setTimeout(resolve, 10))
        return true
      },
    }

    const result = await checkCustomResolverForceResolve(
      [resolver],
      lockfileWithPackages({ 'test-pkg@1.0.0': TEST_PKG_SNAPSHOT })
    )

    expect(result).toBe(true)
  })

  test('runs checks in parallel', async () => {
    const callOrder: string[] = []
    const resolver: CustomResolver = {
      shouldRefreshResolution: async (depPath) => {
        const delays: Record<string, number> = { 'pkg1@1.0.0': 30, 'pkg2@1.0.0': 20 }
        const delay = delays[depPath] ?? 10
        await new Promise(resolve => setTimeout(resolve, delay))
        callOrder.push(depPath)
        return false
      },
    }

    await checkCustomResolverForceResolve(
      [resolver],
      lockfileWithPackages({
        'pkg1@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg1-1.0.0.tgz', integrity: 'sha512-test1' },
        },
        'pkg2@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg2-1.0.0.tgz', integrity: 'sha512-test2' },
        },
        'pkg3@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg3-1.0.0.tgz', integrity: 'sha512-test3' },
        },
      })
    )

    // If parallel, pkg3 finishes first (10ms), then pkg2 (20ms), then pkg1 (30ms)
    expect(callOrder).toEqual(['pkg3@1.0.0', 'pkg2@1.0.0', 'pkg1@1.0.0'])
  })

  test('passes depPath and pkgSnapshot to shouldRefreshResolution', async () => {
    let receivedDepPath: string | undefined
    let receivedPkgSnapshot: unknown
    const resolver: CustomResolver = {
      shouldRefreshResolution: (depPath, pkgSnapshot) => {
        receivedDepPath = depPath
        receivedPkgSnapshot = pkgSnapshot
        return false
      },
    }

    await checkCustomResolverForceResolve(
      [resolver],
      lockfileWithPackages({ 'test-pkg@1.0.0': TEST_PKG_SNAPSHOT })
    )

    expect(receivedDepPath).toBe('test-pkg@1.0.0')
    expect(receivedPkgSnapshot).toEqual(TEST_PKG_SNAPSHOT)
  })

  test('shouldRefreshResolution can filter by depPath to match specific packages', async () => {
    // Resolver uses shouldRefreshResolution to do its own filtering -- this is
    // the expected pattern now that canResolve is not used as a gate.
    const resolver: CustomResolver = {
      canResolve: (wantedDependency) => wantedDependency.alias === 'indirect-pkg',
      shouldRefreshResolution: (depPath) => depPath.startsWith('indirect-pkg@'),
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          specifiers: { 'direct-pkg': '1.0.0' },
          dependencies: { 'direct-pkg': '1.0.0' },
        },
      } as AnyPackages,
      packages: {
        'direct-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/direct-pkg-1.0.0.tgz', integrity: 'sha512-test1' },
          dependencies: { 'indirect-pkg': '2.0.0' },
        },
        'indirect-pkg@2.0.0': {
          resolution: { tarball: 'http://example.com/indirect-pkg-2.0.0.tgz', integrity: 'sha512-test2' },
        },
      } as AnyPackages,
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfile)

    expect(result).toBe(true)
  })

  test('shouldRefreshResolution can inspect pkgSnapshot resolution type', async () => {
    // A resolver that uses the resolution type to decide whether to force
    // re-resolution -- this is the pattern for custom protocol resolvers.
    const resolver: CustomResolver = {
      shouldRefreshResolution: (_depPath, pkgSnapshot) => {
        const resolution = pkgSnapshot.resolution as Record<string, unknown>
        return resolution.type === 'custom:cdn'
      },
    }

    const result = await checkCustomResolverForceResolve(
      [resolver],
      lockfileWithPackages({
        'foo@1.0.0': {
          resolution: { type: 'custom:cdn', source: 'foo' },
        },
        'bar@2.0.0': {
          resolution: { integrity: 'sha512-regular' },
        },
      })
    )

    expect(result).toBe(true)
  })

  test('re-throws errors from shouldRefreshResolution', async () => {
    const resolver: CustomResolver = {
      shouldRefreshResolution: () => {
        throw new Error('resolver crashed')
      },
    }

    await expect(
      checkCustomResolverForceResolve(
        [resolver],
        lockfileWithPackages({ 'test-pkg@1.0.0': TEST_PKG_SNAPSHOT })
      )
    ).rejects.toThrow('resolver crashed')
  })

  test('re-throws even when other resolvers return false', async () => {
    const goodResolver: CustomResolver = {
      shouldRefreshResolution: () => false,
    }
    const badResolver: CustomResolver = {
      shouldRefreshResolution: () => {
        throw new Error('unexpected failure')
      },
    }

    await expect(
      checkCustomResolverForceResolve(
        [goodResolver, badResolver],
        lockfileWithPackages({ 'test-pkg@1.0.0': TEST_PKG_SNAPSHOT })
      )
    ).rejects.toThrow('unexpected failure')
  })
})
