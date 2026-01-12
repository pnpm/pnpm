import { calcGraphNodeHash, type DepsGraph, type DepsStateCache, type PkgMeta } from '@pnpm/calc-dep-state'
import { ENGINE_NAME } from '@pnpm/constants'
import { hashObjectWithoutSorting, hashObject } from '@pnpm/crypto.object-hasher'
import { type DepPath, type PkgIdWithPatchHash } from '@pnpm/types'

describe('calcGraphNodeHash', () => {
  it('should return correct hash format for unscoped package', () => {
    const graph: DepsGraph<DepPath> = {
      ['foo@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'foo@1.0.0:sha512-abc',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'foo@1.0.0' as DepPath,
      name: 'foo',
      version: '1.0.0',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    // Unscoped packages should have @/ prefix
    expect(result).toMatch(/^@\/foo\/1\.0\.0\/[a-f0-9]+$/)
  })

  it('should return correct hash format for scoped package', () => {
    const graph: DepsGraph<DepPath> = {
      ['@scope/bar@2.0.0' as DepPath]: {
        children: {},
        fullPkgId: '@scope/bar@2.0.0:sha512-xyz',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: '@scope/bar@2.0.0' as DepPath,
      name: '@scope/bar',
      version: '2.0.0',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    // Scoped packages should not have @/ prefix (they already start with @)
    expect(result).toMatch(/^@scope\/bar\/2\.0\.0\/[a-f0-9]+$/)
    expect(result).not.toMatch(/^@\/@scope/)
  })

  it('should compute correct hash based on engine and deps', () => {
    const graph: DepsGraph<DepPath> = {
      ['pkg@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'pkg@1.0.0:sha512-integrity',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'pkg@1.0.0' as DepPath,
      name: 'pkg',
      version: '1.0.0',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    // Calculate expected hash manually
    const depsHash = hashObject({
      id: 'pkg@1.0.0:sha512-integrity',
      deps: {},
    })
    const expectedHash = hashObjectWithoutSorting(
      { engine: ENGINE_NAME, deps: depsHash },
      { encoding: 'hex' }
    )

    expect(result).toBe(`@/pkg/1.0.0/${expectedHash}`)
  })

  it('should include dependency hashes in the computation', () => {
    const graph: DepsGraph<DepPath> = {
      ['parent@1.0.0' as DepPath]: {
        children: {
          child: 'child@2.0.0' as DepPath,
        },
        fullPkgId: 'parent@1.0.0:sha512-parent',
      },
      ['child@2.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'child@2.0.0:sha512-child',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'parent@1.0.0' as DepPath,
      name: 'parent',
      version: '1.0.0',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    // Calculate expected hash with child dependency
    const childHash = hashObject({
      id: 'child@2.0.0:sha512-child',
      deps: {},
    })
    const parentDepsHash = hashObject({
      id: 'parent@1.0.0:sha512-parent',
      deps: {
        child: childHash,
      },
    })
    const expectedHash = hashObjectWithoutSorting(
      { engine: ENGINE_NAME, deps: parentDepsHash },
      { encoding: 'hex' }
    )

    expect(result).toBe(`@/parent/1.0.0/${expectedHash}`)
  })

  it('should use cache for repeated calculations', () => {
    const graph: DepsGraph<DepPath> = {
      ['cached@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'cached@1.0.0:sha512-cached',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'cached@1.0.0' as DepPath,
      name: 'cached',
      version: '1.0.0',
    }

    const result1 = calcGraphNodeHash({ graph, cache }, pkgMeta)
    const result2 = calcGraphNodeHash({ graph, cache }, pkgMeta)

    expect(result1).toBe(result2)
    // Cache should have been populated
    expect(cache['cached@1.0.0']).toBeDefined()
  })

  it('should handle circular dependencies', () => {
    const graph: DepsGraph<DepPath> = {
      ['a@1.0.0' as DepPath]: {
        children: {
          b: 'b@1.0.0' as DepPath,
        },
        fullPkgId: 'a@1.0.0:sha512-a',
      },
      ['b@1.0.0' as DepPath]: {
        children: {
          a: 'a@1.0.0' as DepPath,
        },
        fullPkgId: 'b@1.0.0:sha512-b',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'a@1.0.0' as DepPath,
      name: 'a',
      version: '1.0.0',
    }

    // Should not throw or infinite loop
    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    expect(result).toMatch(/^@\/a\/1\.0\.0\/[a-f0-9]+$/)
  })

  it('should handle deeply nested dependencies', () => {
    const graph: DepsGraph<DepPath> = {
      ['level1@1.0.0' as DepPath]: {
        children: {
          level2: 'level2@1.0.0' as DepPath,
        },
        fullPkgId: 'level1@1.0.0:sha512-1',
      },
      ['level2@1.0.0' as DepPath]: {
        children: {
          level3: 'level3@1.0.0' as DepPath,
        },
        fullPkgId: 'level2@1.0.0:sha512-2',
      },
      ['level3@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'level3@1.0.0:sha512-3',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'level1@1.0.0' as DepPath,
      name: 'level1',
      version: '1.0.0',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    expect(result).toMatch(/^@\/level1\/1\.0\.0\/[a-f0-9]+$/)
  })

  it('should produce different hashes for different dependency structures', () => {
    const graph1: DepsGraph<DepPath> = {
      ['pkg@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'pkg@1.0.0:sha512-abc',
      },
    }
    const graph2: DepsGraph<DepPath> = {
      ['pkg@1.0.0' as DepPath]: {
        children: {
          dep: 'dep@1.0.0' as DepPath,
        },
        fullPkgId: 'pkg@1.0.0:sha512-abc',
      },
      ['dep@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'dep@1.0.0:sha512-dep',
      },
    }
    const pkgMeta: PkgMeta = {
      depPath: 'pkg@1.0.0' as DepPath,
      name: 'pkg',
      version: '1.0.0',
    }

    const result1 = calcGraphNodeHash({ graph: graph1, cache: {} }, pkgMeta)
    const result2 = calcGraphNodeHash({ graph: graph2, cache: {} }, pkgMeta)

    expect(result1).not.toBe(result2)
  })

  it('should produce different hashes for different package integrities', () => {
    const graph1: DepsGraph<DepPath> = {
      ['pkg@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'pkg@1.0.0:sha512-integrity1',
      },
    }
    const graph2: DepsGraph<DepPath> = {
      ['pkg@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'pkg@1.0.0:sha512-integrity2',
      },
    }
    const pkgMeta: PkgMeta = {
      depPath: 'pkg@1.0.0' as DepPath,
      name: 'pkg',
      version: '1.0.0',
    }

    const result1 = calcGraphNodeHash({ graph: graph1, cache: {} }, pkgMeta)
    const result2 = calcGraphNodeHash({ graph: graph2, cache: {} }, pkgMeta)

    expect(result1).not.toBe(result2)
  })

  it('should handle multiple children dependencies', () => {
    const graph: DepsGraph<DepPath> = {
      ['root@1.0.0' as DepPath]: {
        children: {
          dep1: 'dep1@1.0.0' as DepPath,
          dep2: 'dep2@1.0.0' as DepPath,
          dep3: 'dep3@1.0.0' as DepPath,
        },
        fullPkgId: 'root@1.0.0:sha512-root',
      },
      ['dep1@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'dep1@1.0.0:sha512-dep1',
      },
      ['dep2@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'dep2@1.0.0:sha512-dep2',
      },
      ['dep3@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'dep3@1.0.0:sha512-dep3',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'root@1.0.0' as DepPath,
      name: 'root',
      version: '1.0.0',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    expect(result).toMatch(/^@\/root\/1\.0\.0\/[a-f0-9]+$/)
  })

  it('should use pkgIdWithPatchHash and resolution when fullPkgId is not defined', () => {
    const graph: DepsGraph<DepPath> = {
      ['pkg@1.0.0' as DepPath]: {
        children: {},
        pkgIdWithPatchHash: 'pkg@1.0.0' as PkgIdWithPatchHash,
        resolution: {
          integrity: 'sha512-abc123',
        },
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'pkg@1.0.0' as DepPath,
      name: 'pkg',
      version: '1.0.0',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    expect(result).toMatch(/^@\/pkg\/1\.0\.0\/[a-f0-9]+$/)
  })

  it('should handle resolution without integrity (hashes the resolution object)', () => {
    const graph: DepsGraph<DepPath> = {
      ['git-pkg@1.0.0' as DepPath]: {
        children: {},
        pkgIdWithPatchHash: 'git-pkg@1.0.0' as PkgIdWithPatchHash,
        resolution: {
          tarball: 'https://github.com/example/repo/archive/v1.0.0.tar.gz',
        },
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'git-pkg@1.0.0' as DepPath,
      name: 'git-pkg',
      version: '1.0.0',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    expect(result).toMatch(/^@\/git-pkg\/1\.0\.0\/[a-f0-9]+$/)
  })

  it('should handle complex scoped package names', () => {
    const graph: DepsGraph<DepPath> = {
      ['@my-org/my-package@1.2.3' as DepPath]: {
        children: {},
        fullPkgId: '@my-org/my-package@1.2.3:sha512-xyz',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: '@my-org/my-package@1.2.3' as DepPath,
      name: '@my-org/my-package',
      version: '1.2.3',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    expect(result).toMatch(/^@my-org\/my-package\/1\.2\.3\/[a-f0-9]+$/)
  })

  it('should handle prerelease versions', () => {
    const graph: DepsGraph<DepPath> = {
      ['pkg@1.0.0-beta.1' as DepPath]: {
        children: {},
        fullPkgId: 'pkg@1.0.0-beta.1:sha512-pre',
      },
    }
    const cache: DepsStateCache = {}
    const pkgMeta: PkgMeta = {
      depPath: 'pkg@1.0.0-beta.1' as DepPath,
      name: 'pkg',
      version: '1.0.0-beta.1',
    }

    const result = calcGraphNodeHash({ graph, cache }, pkgMeta)

    expect(result).toMatch(/^@\/pkg\/1\.0\.0-beta\.1\/[a-f0-9]+$/)
  })

  it('should produce consistent results across multiple calls with same input', () => {
    const graph: DepsGraph<DepPath> = {
      ['consistent@1.0.0' as DepPath]: {
        children: {
          dep: 'dep@1.0.0' as DepPath,
        },
        fullPkgId: 'consistent@1.0.0:sha512-consistent',
      },
      ['dep@1.0.0' as DepPath]: {
        children: {},
        fullPkgId: 'dep@1.0.0:sha512-dep',
      },
    }
    const pkgMeta: PkgMeta = {
      depPath: 'consistent@1.0.0' as DepPath,
      name: 'consistent',
      version: '1.0.0',
    }

    // Use fresh caches each time to ensure determinism isn't from cache
    const results = Array.from({ length: 5 }, () =>
      calcGraphNodeHash({ graph, cache: {} }, pkgMeta)
    )

    expect(new Set(results).size).toBe(1)
  })
})
