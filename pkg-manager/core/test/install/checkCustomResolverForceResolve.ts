import { checkCustomResolverForceResolve } from '../../src/install/checkCustomResolverForceResolve.js'
import { type CustomResolver } from '@pnpm/hooks.types'
import { type LockfileObject } from '@pnpm/lockfile.types'

describe('checkCustomResolverForceResolve', () => {
  test('returns false when no custom resolvers provided', async () => {
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
    }

    const result = await checkCustomResolverForceResolve([], lockfile)

    expect(result).toBe(false)
  })

  test('returns false when lockfile has no packages', async () => {
    const resolver: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfile)

    expect(result).toBe(false)
  })

  test('returns false when custom resolver canResolve returns false', async () => {
    const resolver: CustomResolver = {
      canResolve: () => false,
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfile)

    expect(result).toBe(false)
  })

  test('returns false when custom resolver has no shouldForceResolve', async () => {
    const resolver: CustomResolver = {
      canResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfile)

    expect(result).toBe(false)
  })

  test('returns false when shouldForceResolve returns false', async () => {
    const resolver: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: () => false,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfile)

    expect(result).toBe(false)
  })

  test('returns true when shouldForceResolve returns true', async () => {
    const resolver: CustomResolver = {
      canResolve: (wantedDependency) => wantedDependency.alias === 'test-pkg',
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfile)

    expect(result).toBe(true)
  })

  test('checks scoped packages', async () => {
    const resolver: CustomResolver = {
      canResolve: (wantedDependency) => wantedDependency.alias === '@scope/pkg',
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        '@scope/pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfile)

    expect(result).toBe(true)
  })

  test('handles multiple custom resolvers - first matching returns true', async () => {
    const resolver1: CustomResolver = {
      canResolve: () => false,
      shouldForceResolve: () => true,
    }
    const resolver2: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const result = await checkCustomResolverForceResolve([resolver1, resolver2], lockfile)

    expect(result).toBe(true)
  })

  test('handles async shouldForceResolve', async () => {
    const resolver: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: async () => {
        await new Promise(resolve => setTimeout(resolve, 10))
        return true
      },
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfile)

    expect(result).toBe(true)
  })

  test('runs checks in parallel', async () => {
    const callOrder: string[] = []
    const resolver: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: async (depPath) => {
        // Stagger completion times to verify parallel execution
        const delay = depPath === 'pkg1@1.0.0' ? 30 : depPath === 'pkg2@1.0.0' ? 20 : 10
        await new Promise(resolve => setTimeout(resolve, delay))
        callOrder.push(depPath)
        return false
      },
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'pkg1@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg1-1.0.0.tgz', integrity: 'sha512-test1' },
        },
        'pkg2@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg2-1.0.0.tgz', integrity: 'sha512-test2' },
        },
        'pkg3@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg3-1.0.0.tgz', integrity: 'sha512-test3' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    await checkCustomResolverForceResolve([resolver], lockfile)

    // If parallel, pkg3 finishes first (10ms), then pkg2 (20ms), then pkg1 (30ms)
    expect(callOrder).toEqual(['pkg3@1.0.0', 'pkg2@1.0.0', 'pkg1@1.0.0'])
  })

  test('passes depPath and pkgSnapshot to shouldForceResolve', async () => {
    let receivedDepPath: string | undefined
    let receivedPkgSnapshot: unknown
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        'test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    const resolver: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: (depPath, pkgSnapshot) => {
        receivedDepPath = depPath
        receivedPkgSnapshot = pkgSnapshot
        return false
      },
    }

    await checkCustomResolverForceResolve([resolver], lockfile)

    expect(receivedDepPath).toBe('test-pkg@1.0.0')
    expect(receivedPkgSnapshot).toEqual({
      resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
    })
  })

  test('checks indirect (transitive) dependencies', async () => {
    const resolver: CustomResolver = {
      canResolve: (wantedDependency) => wantedDependency.alias === 'indirect-pkg',
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          specifiers: { 'direct-pkg': '1.0.0' },
          dependencies: { 'direct-pkg': '1.0.0' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packages: {
        'direct-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/direct-pkg-1.0.0.tgz', integrity: 'sha512-test1' },
          dependencies: { 'indirect-pkg': '2.0.0' },
        },
        'indirect-pkg@2.0.0': {
          resolution: { tarball: 'http://example.com/indirect-pkg-2.0.0.tgz', integrity: 'sha512-test2' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }

    const result = await checkCustomResolverForceResolve([resolver], lockfile)

    expect(result).toBe(true)
  })
})
