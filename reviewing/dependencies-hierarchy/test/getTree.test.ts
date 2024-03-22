import { refToRelative } from '@pnpm/dependency-path'
import { type PackageSnapshots } from '@pnpm/lockfile-file'
import { type PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'
import { getTree } from '../lib/getTree'
import { type TreeNodeId } from '../lib/TreeNodeId'

/**
 * Maps an npm package name to its dependencies.
 */
interface MockPackages {
  [pkgName: string]: readonly string[]
}

/**
 * Creates a mock {@see PackageSnapshots} object for easier dependencies
 * hierarchy testing.
 *
 * All packages in the resulting object use the same version since the hierarchy
 * tests usually don't care about version.
 */
function generateMockCurrentPackages (version: string, mockPackages: MockPackages): PackageSnapshots {
  const currentPackages: PackageSnapshots = {}

  for (const [pkgName, dependencies] of Object.entries(mockPackages)) {
    currentPackages[refToRelativeOrThrow(version, pkgName)] = {
      resolution: { integrity: `${pkgName}-mock-integrity-for-testing` },
      dependencies: Object.fromEntries(dependencies.map(depName => [depName, refToRelativeOrThrow(version, depName)])),
    }
  }

  // Add the package name of any dependencies not explicitly specified to make
  // this function easier to use.
  const unspecifiedDepPaths = Object.values(mockPackages)
    .flat()
    .map(pkgName => refToRelativeOrThrow(version, pkgName))
    .filter(key => currentPackages[key] == null)
  for (const depPath of unspecifiedDepPaths) {
    currentPackages[depPath] = {
      resolution: { integrity: `${depPath}-mock-integrity-for-testing` },
      dependencies: {},
    }
  }

  return currentPackages
}

function refToRelativeOrThrow (reference: string, pkgName: string): string {
  const relative = refToRelative(reference, pkgName)
  if (relative == null) {
    throw new Error(`Unable to create key for ${pkgName} with reference ${reference}`)
  }
  return relative
}

/**
 * If {@see PackageNode} has no dependencies, the `dependencies` field is not
 * set at all.
 *
 * This is usually desirable. However, Jest structural matchers currently have
 * no way of asserting that a field is unset. This utility function recursively
 * sets the `dependencies` field to `undefined` if it is not set, which is a
 * workaround allowing test to do:
 *
 * ```ts
 * expect(node).toEqual(expect.objectContaining({ dependencies: undefined }))
 * ```
 */
function normalizePackageNodeForTesting (nodes: readonly PackageNode[]): PackageNode[] {
  return nodes.map(node => ({
    ...node,
    dependencies: node.dependencies != null ? normalizePackageNodeForTesting(node.dependencies) : undefined,
  }))
}

describe('getTree', () => {
  describe('prints at expected depth', () => {
    const version = '1.0.0'
    const currentPackages = generateMockCurrentPackages(version, {
      a: ['b1', 'b2', 'b3'],
      b1: ['c1'],
      c1: ['d1'],
    })
    const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'a') }

    const getTreeArgs = {
      maxDepth: 0,
      rewriteLinkVersionDir: '',
      virtualStoreDir: '.pnpm',
      importers: {},
      includeOptionalDependencies: false,
      lockfileDir: '',
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
      currentPackages,
      wantedPackages: currentPackages,
      lockfile: { lockfileVersion: '7.0', importers: {} },
    }

    test('full test case to print when max depth is large', () => {
      const result = normalizePackageNodeForTesting(getTree({ ...getTreeArgs, maxDepth: 9999 }, rootNodeId))

      expect(result).toEqual([
        expect.objectContaining({
          alias: 'b1',
          dependencies: [
            expect.objectContaining({
              alias: 'c1',
              dependencies: [
                expect.objectContaining({ alias: 'd1', dependencies: undefined }),
              ],
            }),
          ],
        }),
        expect.objectContaining({ alias: 'b2', dependencies: undefined }),
        expect.objectContaining({ alias: 'b3', dependencies: undefined }),
      ])
    })

    test('no result when current depth exceeds max depth', () => {
      const result = getTree({ ...getTreeArgs, maxDepth: 0 }, rootNodeId)
      expect(result).toEqual([])
    })

    test('max depth of 1 to print flat dependencies', () => {
      const result = getTree({ ...getTreeArgs, maxDepth: 1 }, rootNodeId)

      expect(normalizePackageNodeForTesting(result)).toEqual([
        expect.objectContaining({ alias: 'b1', dependencies: undefined }),
        expect.objectContaining({ alias: 'b2', dependencies: undefined }),
        expect.objectContaining({ alias: 'b3', dependencies: undefined }),
      ])
    })

    test('max depth of 2 to print a1 -> b1 -> c1, but not d1', () => {
      const result = getTree({ ...getTreeArgs, maxDepth: 2 }, rootNodeId)

      expect(normalizePackageNodeForTesting(result)).toEqual([
        expect.objectContaining({
          alias: 'b1',
          dependencies: [
            expect.objectContaining({
              alias: 'c1',
              // c1 has a dependency on d1, but it should not be printed.
              dependencies: undefined,
            }),
          ],
        }),
        expect.objectContaining({ alias: 'b2', dependencies: undefined }),
        expect.objectContaining({ alias: 'b3', dependencies: undefined }),
      ])
    })
  })

  // This group of tests attempts to check that package tree caching still
  // respects max depth. See https://github.com/pnpm/pnpm/issues/4814
  //
  // This doesn't test the cache directly, but sets up situations that would
  // result in incorrect output if the cache was used when it's not supposed to.
  describe('prints at expected depth for cache regression testing cases', () => {
    const commonMockGetTreeArgs = {
      rewriteLinkVersionDir: '',
      modulesDir: '',
      importers: {},
      includeOptionalDependencies: false,
      lockfileDir: '',
      lockfile: { lockfileVersion: '7.0', importers: {} },
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
    }

    test('revisiting package at lower depth prints dependencies not previously printed', () => {
      // This tests the "glob" npm package on a subset of its dependency tree.
      // Max depth shown in square brackets.
      //
      // root
      // └─┬ glob [2]
      //   ├─┬ inflight [1]
      //   │ └── once [0]    <-- 1st time seen. No dependencies of "once" printed due to max depth.
      //   └─┬ once [1]      <-- 2nd time seen, but at different depth. The "wrappy" dependency below should be printed.
      //     └── wrappy [0]
      //
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['glob'],
        glob: ['inflight', 'once'],
        inflight: ['once'],
        once: ['wrappy'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const result = getTree({
        ...commonMockGetTreeArgs,
        maxDepth: 3,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizePackageNodeForTesting(result)).toEqual([
        // depth 0
        expect.objectContaining({
          alias: 'glob',
          dependencies: expect.arrayContaining([

            // depth 1
            expect.objectContaining({
              alias: 'inflight',
              dependencies: expect.arrayContaining([
                // depth 2
                expect.objectContaining({
                  // The "once" package is first seen here at depth 2.
                  alias: 'once',
                }),
              ]),
            }),

            // depth 1
            expect.objectContaining({
              alias: 'once',
              dependencies: [
                // The "once" package is seen again at depth 1. The "once"
                // package contains a "wrappy" package that should be listed.
                expect.objectContaining({ alias: 'wrappy' }),
              ],
            }),
          ]),
        }),
      ])
    })

    test('revisiting package at higher depth does not print extra dependencies', () => {
      // This tests the "glob" npm package on a subset of its dependency tree.
      // Max depth shown in square brackets.
      //
      // root
      // └─┬ a [2]
      //   ├─┬ b [1]   <-- 1st time "b" is seen.
      //   │ └── c [0]
      //   └─┬ d [1]
      //     └── b [0] <-- 2nd time "b" is seen. Dependencies should not be printed since "max depth === current depth".
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a'],
        a: ['b', 'd'],
        b: ['c'],
        d: ['b'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const result = getTree({
        ...commonMockGetTreeArgs,
        maxDepth: 3,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizePackageNodeForTesting(result)).toEqual([
        expect.objectContaining({
          alias: 'a',
          dependencies: [

            expect.objectContaining({
              alias: 'b',
              dependencies: [
                expect.objectContaining({
                  alias: 'c',
                }),
              ],
            }),

            expect.objectContaining({
              alias: 'd',
              dependencies: [
                expect.objectContaining({
                  alias: 'b',

                  // The "b" package has a "c" dependency, but it should not be
                  // printed since the max depth was reached.
                  dependencies: undefined,
                }),
              ],
            }),
          ],
        }),
      ])
    })
  })

  // This group of tests attempts to check that situations when the "fully
  // visited cache" can be reused is correct.
  //
  // This doesn't test the cache directly, but sets up situations that would
  // result in incorrect output if the cache was used when it's not supposed to.
  describe('fully visited cache optimization handles requested depth correctly', () => {
    const commonMockGetTreeArgs = {
      rewriteLinkVersionDir: '',
      modulesDir: '',
      importers: {},
      includeOptionalDependencies: false,
      lockfileDir: '',
      lockfile: { lockfileVersion: '7.0', importers: {} },
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
    }

    // The fully visited cache can be used in this situation.
    test('height < requestedDepth', () => {
      // Max depth shown in square brackets.
      //
      // root
      // ├─┬ a [3]
      // │ └─┬ b [2]   <-- 1st time "b" is seen, its dependencies are recorded to the cache with a height of 1.
      // │   └── c [1] <-- Max depth remaining must be >=1 for parent nodes to enter the fully visited cache.
      // └─┬ b [3]     <-- 2nd time "b" is seen. Cache should be reused since requested depth is 3.
      //   └── c [2]
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a', 'b'],
        a: ['b'],
        b: ['c'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const result = getTree({
        ...commonMockGetTreeArgs,
        maxDepth: 4,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizePackageNodeForTesting(result)).toEqual([
        expect.objectContaining({
          alias: 'a',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              dependencies: [
                expect.objectContaining({
                  alias: 'c',
                }),
              ],
            }),
          ],
        }),
        expect.objectContaining({
          alias: 'b',
          dependencies: [
            expect.objectContaining({
              alias: 'c',
              dependencies: undefined,
            }),
          ],
        }),
      ])
    })

    test('height === requestedDepth', () => {
      // Max depth shown in square brackets.
      //
      // root
      // ├─┬ a [3]       <-- 1st time "a" is seen, its dependencies are recorded to the cache with a height of 1.
      // │ └── b [2]     <-- Max depth remaining must be >=1 for parent nodes to enter the fully visited cache.
      // └─┬ c [3]
      //   └─┬ d [2]
      //     └─┬ a [1]   <-- 2nd time "a" is seen. Cache should be reused since requested depth is 1 and height is 1.
      //       └── b [0]
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a', 'c'],
        a: ['b'],
        c: ['d'],
        d: ['a'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const result = getTree({
        ...commonMockGetTreeArgs,
        maxDepth: 4,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      const expectedA = expect.objectContaining({
        alias: 'a',
        dependencies: [
          expect.objectContaining({
            alias: 'b',
            dependencies: undefined,
          }),
        ],
      })

      expect(normalizePackageNodeForTesting(result)).toEqual([
        expectedA,
        expect.objectContaining({
          alias: 'c',
          dependencies: [
            expect.objectContaining({
              alias: 'd',
              dependencies: [
                expectedA,
              ],
            }),
          ],
        }),
      ])
    })

    test('height === requestedDepth + 1', () => {
      // Max depth shown in square brackets.
      //
      // root [3]
      // ├─┬ a [2]      <-- 1st time "a" is seen. Its dependencies are recorded to the cache with a height of 1.
      // │ └── b [1]    <-- Max depth remaining must be >=1 for parent nodes to enter the fully visited cache.
      // └─┬ c [2]
      //   └─┬ d [1]
      //     └── a [0]  <-- 2nd time "a" is seen. Cache should not be reused since requested depth is 0 and height is 1.
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a', 'c'],
        a: ['b'],
        c: ['d'],
        d: ['a'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const result = getTree({
        ...commonMockGetTreeArgs,
        maxDepth: 3,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizePackageNodeForTesting(result)).toEqual([
        expect.objectContaining({
          alias: 'a',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              dependencies: undefined,
            }),
          ],
        }),
        expect.objectContaining({
          alias: 'c',
          dependencies: [
            expect.objectContaining({
              alias: 'd',
              dependencies: [
                expect.objectContaining({
                  alias: 'a',
                  // The "a" dependency has more dependencies, but they
                  // should not be printed to respect max depth.
                  dependencies: undefined,
                }),
              ],
            }),
          ],
        }),
      ])
    })

    test('height > requestedDepth', () => {
      // Max depth shown in square brackets.
      //
      // root [5]
      // ├─┬ a [4]         <-- 1st time "a" is seen. Its dependencies are recorded to the cache with a height of 3.
      // │ └─┬ b [3]
      // │   └─┬ c [2]
      // │     └── d [1]   <-- Max depth remaining must be >=1 for parent nodes to enter the fully visited cache.
      // └─┬ e [4]
      //   └─┬ f [3]
      //     └─┬ g [2]
      //       └─┬ a [1]   <-- 2nd time "a" is seen. Cache should not be used.
      //         └── b [0]
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a', 'e'],
        a: ['b'],
        b: ['c'],
        c: ['d'],
        e: ['f'],
        f: ['g'],
        g: ['a'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const result = getTree({
        ...commonMockGetTreeArgs,
        maxDepth: 5,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizePackageNodeForTesting(result)).toEqual([
        expect.objectContaining({
          alias: 'a',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              dependencies: [
                expect.objectContaining({
                  alias: 'c',
                  dependencies: [
                    expect.objectContaining({
                      alias: 'd',
                      dependencies: undefined,
                    }),
                  ],
                }),
              ],
            }),
          ],
        }),
        expect.objectContaining({
          alias: 'e',
          dependencies: [
            expect.objectContaining({
              alias: 'f',
              dependencies: [
                expect.objectContaining({
                  alias: 'g',
                  dependencies: [
                    expect.objectContaining({
                      alias: 'a',
                      dependencies: [
                        expect.objectContaining({
                          alias: 'b',
                          // The "b" dependency has more dependencies, but they
                          // should not be printed to respect max depth.
                          dependencies: undefined,
                        }),
                      ],
                    }),
                  ],
                }),
              ],
            }),
          ],
        }),
      ])
    })
  })
})
