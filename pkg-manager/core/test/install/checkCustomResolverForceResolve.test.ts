import { getCustomResolverForceResolveDeps, type ProjectWithManifest } from '../../src/install/checkCustomResolverForceResolve.js'
import { type CustomResolver } from '@pnpm/hooks.types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { type ProjectId } from '@pnpm/types'

describe('getCustomResolverForceResolveDeps', () => {
  test('returns empty set when no custom resolvers provided', async () => {
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
    }
    const projects: ProjectWithManifest[] = []

    const result = await getCustomResolverForceResolveDeps([], lockfile, projects)

    expect(result.size).toBe(0)
  })

  test('returns empty set when no projects provided', async () => {
    const resolver: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
    }

    const result = await getCustomResolverForceResolveDeps([resolver], lockfile, [])

    expect(result.size).toBe(0)
  })

  test('returns empty set when custom resolver canResolve returns false', async () => {
    const resolver: CustomResolver = {
      canResolve: () => false,
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          dependencies: {
            'test-pkg': '/test-pkg@1.0.0',
          },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packages: {
        '/test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    const projects: ProjectWithManifest[] = [
      {
        id: '.' as ProjectId,
        manifest: {
          dependencies: {
            'test-pkg': '1.0.0',
          },
        } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      },
    ]

    const result = await getCustomResolverForceResolveDeps([resolver], lockfile, projects)

    expect(result.size).toBe(0)
  })

  test('returns empty set when custom resolver has no shouldForceResolve', async () => {
    const resolver: CustomResolver = {
      canResolve: () => true,
      // No shouldForceResolve
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          dependencies: {
            'test-pkg': '/test-pkg@1.0.0',
          },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packages: {
        '/test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    const projects: ProjectWithManifest[] = [
      {
        id: '.' as ProjectId,
        manifest: {
          dependencies: {
            'test-pkg': '1.0.0',
          },
        } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      },
    ]

    const result = await getCustomResolverForceResolveDeps([resolver], lockfile, projects)

    expect(result.size).toBe(0)
  })

  test('returns empty set when shouldForceResolve returns false', async () => {
    const resolver: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: () => false,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          dependencies: {
            'test-pkg': '/test-pkg@1.0.0',
          },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packages: {
        '/test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    const projects: ProjectWithManifest[] = [
      {
        id: '.' as ProjectId,
        manifest: {
          dependencies: {
            'test-pkg': '1.0.0',
          },
        } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      },
    ]

    const result = await getCustomResolverForceResolveDeps([resolver], lockfile, projects)

    expect(result.size).toBe(0)
  })

  test('returns set with dep name when shouldForceResolve returns true', async () => {
    const resolver: CustomResolver = {
      canResolve: (wantedDependency) => wantedDependency.alias === 'test-pkg',
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          dependencies: {
            'test-pkg': '/test-pkg@1.0.0',
          },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packages: {
        '/test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    const projects: ProjectWithManifest[] = [
      {
        id: '.' as ProjectId,
        manifest: {
          dependencies: {
            'test-pkg': '1.0.0',
          },
        } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      },
    ]

    const result = await getCustomResolverForceResolveDeps([resolver], lockfile, projects)

    expect(result.has('test-pkg')).toBe(true)
    expect(result.size).toBe(1)
  })

  test('returns empty set when custom resolver has no canResolve method', async () => {
    const resolver: CustomResolver = {
      // No canResolve method at all
      shouldForceResolve: () => true,
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          dependencies: {
            'test-pkg': '/test-pkg@1.0.0',
          },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packages: {
        '/test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    const projects: ProjectWithManifest[] = [
      {
        id: '.' as ProjectId,
        manifest: {
          dependencies: {
            'test-pkg': '1.0.0',
          },
        } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      },
    ]

    const result = await getCustomResolverForceResolveDeps([resolver], lockfile, projects)

    expect(result.size).toBe(0)
  })

  test('passes lockfile to shouldForceResolve', async () => {
    let receivedLockfile: LockfileObject | undefined
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          specifiers: { 'test-pkg': '1.0.0' },
          dependencies: { 'test-pkg': '1.0.0' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packages: {
        '/test-pkg@1.0.0': {
          resolution: { tarball: 'http://example.com/test-pkg-1.0.0.tgz', integrity: 'sha512-test' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    const resolver: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: (_wantedDep, wantedLockfile) => {
        receivedLockfile = wantedLockfile
        return false
      },
    }
    const projects: ProjectWithManifest[] = [
      {
        id: '.' as ProjectId,
        manifest: {
          dependencies: { 'test-pkg': '1.0.0' },
        } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      },
    ]

    await getCustomResolverForceResolveDeps([resolver], lockfile, projects)

    expect(receivedLockfile).toBe(lockfile)
  })

  test('collects all deps that need force resolution', async () => {
    const resolver: CustomResolver = {
      canResolve: () => true,
      shouldForceResolve: (wantedDep) => wantedDep.alias === 'pkg1' || wantedDep.alias === 'pkg3',
    }
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          dependencies: {
            pkg1: '/pkg1@1.0.0',
            pkg2: '/pkg2@1.0.0',
            pkg3: '/pkg3@1.0.0',
          },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      packages: {
        '/pkg1@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg1-1.0.0.tgz', integrity: 'sha512-test1' },
        },
        '/pkg2@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg2-1.0.0.tgz', integrity: 'sha512-test2' },
        },
        '/pkg3@1.0.0': {
          resolution: { tarball: 'http://example.com/pkg3-1.0.0.tgz', integrity: 'sha512-test3' },
        },
      } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    const projects: ProjectWithManifest[] = [
      {
        id: '.' as ProjectId,
        manifest: {
          dependencies: {
            pkg1: '1.0.0',
            pkg2: '1.0.0',
            pkg3: '1.0.0',
          },
        } as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      },
    ]

    const result = await getCustomResolverForceResolveDeps([resolver], lockfile, projects)

    expect(result.has('pkg1')).toBe(true)
    expect(result.has('pkg2')).toBe(false)
    expect(result.has('pkg3')).toBe(true)
    expect(result.size).toBe(2)
  })
})
