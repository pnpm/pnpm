import { refToRelative } from '@pnpm/dependency-path'
import { type PackageSnapshots } from '@pnpm/lockfile.fs'
import { type DependencyNode } from '@pnpm/reviewing.dependencies-hierarchy'
import { type DepPath, type Finder } from '@pnpm/types'
import { buildDependencyGraph } from '../lib/buildDependencyGraph.js'
import { getTree, type MaterializationCache } from '../lib/getTree.js'
import { type TreeNodeId } from '../lib/TreeNodeId.js'

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

function refToRelativeOrThrow (reference: string, pkgName: string): DepPath {
  const relative = refToRelative(reference, pkgName)
  if (relative == null) {
    throw new Error(`Unable to create key for ${pkgName} with reference ${reference}`)
  }
  return relative
}

/**
 * If {@see DependencyNode} has no dependencies, the `dependencies` field is not
 * set at all.
 *
 * This is usually desirable. However, Jest structural matchers currently have
 * no way of asserting that a field is unset. This utility function recursively
 * sets the `dependencies` field to `undefined` if it is not set, which is a
 * workaround allowing test to do:
 *
 * ```ts
 * expect(node).toMatchObject({ dependencies: undefined })
 * ```
 */
function normalizeDependencyNodeForTesting (nodes: readonly DependencyNode[]): DependencyNode[] {
  return nodes.map(node => ({
    ...node,
    dependencies: node.dependencies != null ? normalizeDependencyNodeForTesting(node.dependencies) : undefined,
  }))
}

function getTreeWithGraph (
  opts: Omit<Parameters<typeof getTree>[0], 'graph' | 'materializationCache'>,
  rootNodeId: TreeNodeId
) {
  const graph = buildDependencyGraph([rootNodeId], opts)
  const materializationCache: MaterializationCache = new Map()
  return getTree({ ...opts, graph, materializationCache }, rootNodeId)
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
      depTypes: {},
      maxDepth: 0,
      rewriteLinkVersionDir: '',
      virtualStoreDir: '.pnpm',
      importers: {},
      include: { optionalDependencies: false },
      lockfileDir: '',
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
      currentPackages,
      wantedPackages: currentPackages,
    }

    test('full test case to print when max depth is large', () => {
      const result = normalizeDependencyNodeForTesting(getTreeWithGraph({ ...getTreeArgs, maxDepth: 9999, virtualStoreDirMaxLength: 120 }, rootNodeId))

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
      const result = getTreeWithGraph({ ...getTreeArgs, maxDepth: 0, virtualStoreDirMaxLength: 120 }, rootNodeId)
      expect(result).toEqual([])
    })

    test('max depth of 1 to print flat dependencies', () => {
      const result = getTreeWithGraph({ ...getTreeArgs, maxDepth: 1, virtualStoreDirMaxLength: 120 }, rootNodeId)

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
        expect.objectContaining({ alias: 'b1', dependencies: undefined }),
        expect.objectContaining({ alias: 'b2', dependencies: undefined }),
        expect.objectContaining({ alias: 'b3', dependencies: undefined }),
      ])
    })

    test('max depth of 2 to print a1 -> b1 -> c1, but not d1', () => {
      const result = getTreeWithGraph({ ...getTreeArgs, maxDepth: 2, virtualStoreDirMaxLength: 120 }, rootNodeId)

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
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
      depTypes: {},
      rewriteLinkVersionDir: '',
      modulesDir: '',
      importers: {},
      include: { optionalDependencies: false },
      lockfileDir: '',
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

      const result = getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: 3,
        currentPackages,
        wantedPackages: currentPackages,
        virtualStoreDirMaxLength: 120,
      }, rootNodeId)

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
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

      const result = getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: 3,
        currentPackages,
        wantedPackages: currentPackages,
        virtualStoreDirMaxLength: 120,
      }, rootNodeId)

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
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
      depTypes: {},
      rewriteLinkVersionDir: '',
      modulesDir: '',
      importers: {},
      include: { optionalDependencies: false },
      lockfileDir: '',
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
      virtualStoreDirMaxLength: 120,
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

      const result = getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: 4,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
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

      const result = getTreeWithGraph({
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

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
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

      const result = getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: 3,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
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

      const result = getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: 5,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
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
  describe('circular dependency detection', () => {
    const commonMockGetTreeArgs = {
      depTypes: {},
      rewriteLinkVersionDir: '',
      modulesDir: '',
      importers: {},
      include: { optionalDependencies: false },
      lockfileDir: '',
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
      virtualStoreDirMaxLength: 120,
    }

    test('marks back-edge as circular in a simple cycle', () => {
      // root → a → b → a(circular)
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a'],
        a: ['b'],
        b: ['a'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const result = getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
        expect.objectContaining({
          alias: 'a',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              dependencies: [
                expect.objectContaining({
                  alias: 'a',
                  circular: true,
                  dependencies: undefined,
                }),
              ],
            }),
          ],
        }),
      ])
    })

    test('does not mark a node as circular when reached from a non-cyclic path', () => {
      // root → a → b → a(circular)
      // root → c → b(deduped — b was already expanded under a)
      //
      // The node "b" under "c" should be deduped, NOT circular.
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a', 'c'],
        a: ['b'],
        b: ['a'],
        c: ['b'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const result = getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
      }, rootNodeId)

      expect(normalizeDependencyNodeForTesting(result)).toEqual([
        expect.objectContaining({
          alias: 'a',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              dependencies: [
                expect.objectContaining({
                  alias: 'a',
                  circular: true,
                  dependencies: undefined,
                }),
              ],
            }),
          ],
        }),
        expect.objectContaining({
          alias: 'c',
          dependencies: [
            // b is deduped (already expanded under a), not circular.
            expect.not.objectContaining({ circular: true }),
          ],
        }),
      ])
    })
  })

  describe('linked dependencies', () => {
    const lockfileDir = '/project'
    const commonMockGetTreeArgs = {
      depTypes: {},
      rewriteLinkVersionDir: lockfileDir,
      modulesDir: '',
      include: { optionalDependencies: false },
      lockfileDir,
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
      virtualStoreDirMaxLength: 120,
    }

    test('link outside workspace appears as leaf node', () => {
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        'regular-dep': ['transitive'],
      })
      const importers = {
        '.': {
          specifiers: {},
          dependencies: {
            'regular-dep': '1.0.0',
            'my-link': 'link:../external-pkg',
          },
        },
      }
      const rootNodeId: TreeNodeId = { type: 'importer', importerId: '.' }

      const result = normalizeDependencyNodeForTesting(getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
        importers,
      }, rootNodeId))

      expect(result).toEqual(expect.arrayContaining([
        expect.objectContaining({
          alias: 'my-link',
          version: expect.stringContaining('link:'),
          dependencies: undefined,
        }),
        expect.objectContaining({
          alias: 'regular-dep',
          dependencies: [
            expect.objectContaining({ alias: 'transitive', dependencies: undefined }),
          ],
        }),
      ]))
    })

    test('link inside workspace resolves to importer and is traversed', () => {
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        leaf: [],
      })
      const importers = {
        '.': {
          specifiers: {},
          dependencies: {
            'workspace-pkg': 'link:packages/workspace-pkg',
          },
        },
        'packages/workspace-pkg': {
          specifiers: {},
          dependencies: {
            leaf: '1.0.0',
          },
        },
      }
      const rootNodeId: TreeNodeId = { type: 'importer', importerId: '.' }

      const result = normalizeDependencyNodeForTesting(getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
        importers,
      }, rootNodeId))

      expect(result).toEqual([
        expect.objectContaining({
          alias: 'workspace-pkg',
          version: expect.stringContaining('link:'),
          dependencies: [
            expect.objectContaining({ alias: 'leaf', dependencies: undefined }),
          ],
        }),
      ])
    })
  })

  describe('search with deduplication', () => {
    const commonMockGetTreeArgs = {
      depTypes: {},
      rewriteLinkVersionDir: '',
      modulesDir: '',
      importers: {},
      include: { optionalDependencies: false },
      lockfileDir: '',
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
      virtualStoreDirMaxLength: 120,
    }

    test('deduped subtree containing a search match still appears in output', () => {
      // root → a → b → target (search match)
      // root → c → b (deduped, but subtree contains a search match)
      //
      // Without the fix, "c → b" would be excluded because b is deduped
      // (empty deps) and b itself doesn't match the search.
      // With the fix, "c → b" appears as deduped + searched.
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a', 'c'],
        a: ['b'],
        b: ['target'],
        c: ['b'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const search: Finder = ({ name }) => name === 'target'

      const result = normalizeDependencyNodeForTesting(getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
        search,
        showDedupedSearchMatches: true,
      }, rootNodeId))

      expect(result).toEqual([
        expect.objectContaining({
          alias: 'a',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              dependencies: [
                expect.objectContaining({
                  alias: 'target',
                  searched: true,
                }),
              ],
            }),
          ],
        }),
        expect.objectContaining({
          alias: 'c',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              deduped: true,
              searched: true,
              dependencies: undefined,
            }),
          ],
        }),
      ])
    })

    test('deduped subtree propagates string search messages to the deduped node', () => {
      // Same graph as above, but the Finder returns a string message.
      // root → a → b → target (search match with message)
      // root → c → b (deduped — should show the message from target)
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a', 'c'],
        a: ['b'],
        b: ['target'],
        c: ['b'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const search: Finder = ({ name }) => name === 'target' ? 'depends on target' : false

      const result = normalizeDependencyNodeForTesting(getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
        search,
        showDedupedSearchMatches: true,
      }, rootNodeId))

      // The deduped "b" under "c" should carry the search message from "target"
      expect(result).toEqual([
        expect.objectContaining({
          alias: 'a',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              dependencies: [
                expect.objectContaining({
                  alias: 'target',
                  searched: true,
                  searchMessage: 'depends on target',
                }),
              ],
            }),
          ],
        }),
        expect.objectContaining({
          alias: 'c',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              deduped: true,
              searched: true,
              searchMessage: 'depends on target',
              dependencies: undefined,
            }),
          ],
        }),
      ])
    })

    test('deduped subtree with search match is hidden by default', () => {
      // Same graph: root → a → b → target, root → c → b (deduped)
      // Without showDedupedSearchMatches, "c → b" should NOT appear
      // even though b's subtree contains a match.
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a', 'c'],
        a: ['b'],
        b: ['target'],
        c: ['b'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const search: Finder = ({ name }) => name === 'target'

      const result = normalizeDependencyNodeForTesting(getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
        search,
      }, rootNodeId))

      // Only "a → b → target" should appear; "c" is excluded because
      // its only child "b" is deduped and doesn't directly match.
      expect(result).toEqual([
        expect.objectContaining({
          alias: 'a',
          dependencies: [
            expect.objectContaining({
              alias: 'b',
              dependencies: [
                expect.objectContaining({
                  alias: 'target',
                  searched: true,
                }),
              ],
            }),
          ],
        }),
      ])
    })

    test('deduped subtree without search match is excluded when search is active', () => {
      // root → a → b → leaf (no match)
      // root → c → b (deduped, subtree has no search match)
      //
      // When searching for "target" (which doesn't exist), neither a nor c
      // should appear because nothing matches.
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a', 'c'],
        a: ['b'],
        b: ['leaf'],
        c: ['b'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const search: Finder = ({ name }) => name === 'target'

      const result = getTreeWithGraph({
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
        search,
      }, rootNodeId)

      expect(result).toEqual([])
    })
  })

  describe('buildDependencyGraph with multiple roots', () => {
    test('graph includes nodes reachable from all specified root IDs', () => {
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        a: ['shared'],
        b: ['unique-to-b'],
        shared: ['deep'],
      })
      const importers = {
        'project-a': {
          specifiers: {},
          dependencies: {
            a: '1.0.0',
          },
        },
        'project-b': {
          specifiers: {},
          dependencies: {
            b: '1.0.0',
          },
        },
      }
      const rootA: TreeNodeId = { type: 'importer', importerId: 'project-a' }
      const rootB: TreeNodeId = { type: 'importer', importerId: 'project-b' }

      // Build graph from both roots
      const multiGraph = buildDependencyGraph([rootA, rootB], {
        currentPackages,
        importers,
        include: { optionalDependencies: false },
        lockfileDir: '',
      })

      // Build graphs from individual roots for comparison
      const graphA = buildDependencyGraph([rootA], {
        currentPackages,
        importers,
        include: { optionalDependencies: false },
        lockfileDir: '',
      })
      const graphB = buildDependencyGraph([rootB], {
        currentPackages,
        importers,
        include: { optionalDependencies: false },
        lockfileDir: '',
      })

      // Multi-root graph should include all nodes from both individual graphs
      for (const key of graphA.nodes.keys()) {
        expect(multiGraph.nodes.has(key)).toBe(true)
      }
      for (const key of graphB.nodes.keys()) {
        expect(multiGraph.nodes.has(key)).toBe(true)
      }

      // Multi-root graph should include nodes unique to each root
      const allKeys = [...multiGraph.nodes.keys()]
      expect(allKeys.some(k => k.includes('unique-to-b'))).toBe(true)
      expect(allKeys.some(k => k.includes('shared'))).toBe(true)
      expect(allKeys.some(k => k.includes('deep'))).toBe(true)
    })
  })

  describe('cross-call deduplication via shared MaterializationCache', () => {
    const commonMockGetTreeArgs = {
      depTypes: {},
      rewriteLinkVersionDir: '',
      modulesDir: '',
      importers: {},
      include: { optionalDependencies: false },
      lockfileDir: '',
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
      virtualStoreDirMaxLength: 120,
    }

    test('second getTree call for same node returns deduped children', () => {
      // root → a → b → c
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a'],
        a: ['b'],
        b: ['c'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const graph = buildDependencyGraph([rootNodeId], {
        currentPackages,
        importers: {},
        include: { optionalDependencies: false },
        lockfileDir: '',
      })
      // Share a single cache across two calls
      const materializationCache: MaterializationCache = new Map()

      const opts = {
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
        graph,
        materializationCache,
      }

      // First call: full materialization
      const result1 = getTree(opts, rootNodeId)
      expect(result1).toHaveLength(1)
      expect(result1[0].alias).toBe('a')
      expect(result1[0].dependencies).toBeDefined()

      // Second call with same cache: child 'a' should be deduped
      const result2 = getTree(opts, rootNodeId)
      expect(result2).toHaveLength(1)
      expect(result2[0].alias).toBe('a')
      expect(result2[0].deduped).toBe(true)
      expect(result2[0].dedupedDependenciesCount).toBeGreaterThan(0)
      expect(result2[0].dependencies).toBeUndefined()
    })

    test('deduped result preserves search match metadata', () => {
      // root → a → target (search match)
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a'],
        a: ['target'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const graph = buildDependencyGraph([rootNodeId], {
        currentPackages,
        importers: {},
        include: { optionalDependencies: false },
        lockfileDir: '',
      })
      const materializationCache: MaterializationCache = new Map()

      const search: Finder = ({ name }) => name === 'target' ? 'found target' : false

      const opts = {
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
        graph,
        materializationCache,
        search,
        showDedupedSearchMatches: true,
      }

      // First call materializes the tree
      const result1 = getTree(opts, rootNodeId)
      expect(result1).toHaveLength(1)
      expect(result1[0].dependencies?.[0]?.searched).toBe(true)
      expect(result1[0].dependencies?.[0]?.searchMessage).toBe('found target')

      // Second call: 'a' is deduped but carries search metadata from cache
      const result2 = getTree(opts, rootNodeId)
      expect(result2).toHaveLength(1)
      expect(result2[0].deduped).toBe(true)
      expect(result2[0].searched).toBe(true)
      expect(result2[0].searchMessage).toBe('found target')
    })

    test('dedupedDependenciesCount correctly reflects subtree size', () => {
      // root → a → b
      //            └→ c
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a'],
        a: ['b', 'c'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const graph = buildDependencyGraph([rootNodeId], {
        currentPackages,
        importers: {},
        include: { optionalDependencies: false },
        lockfileDir: '',
      })
      const materializationCache: MaterializationCache = new Map()

      const opts = {
        ...commonMockGetTreeArgs,
        maxDepth: Infinity,
        currentPackages,
        wantedPackages: currentPackages,
        graph,
        materializationCache,
      }

      // First call: full materialization
      const result1 = getTree(opts, rootNodeId)
      expect(result1).toHaveLength(1) // just 'a'
      expect(result1[0].dependencies).toHaveLength(2) // b and c

      // Second call: deduped, with correct count
      const result2 = getTree(opts, rootNodeId)
      expect(result2).toHaveLength(1) // just 'a'
      expect(result2[0].deduped).toBe(true)
      // a's subtree had 2 nodes (b and c)
      expect(result2[0].dedupedDependenciesCount).toBe(2)
    })

    test('different maxDepth values are cached independently', () => {
      // root → a → b → c
      const version = '1.0.0'
      const currentPackages = generateMockCurrentPackages(version, {
        root: ['a'],
        a: ['b'],
        b: ['c'],
      })
      const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'root') }

      const graph = buildDependencyGraph([rootNodeId], {
        currentPackages,
        importers: {},
        include: { optionalDependencies: false },
        lockfileDir: '',
      })
      const materializationCache: MaterializationCache = new Map()

      const baseOpts = {
        ...commonMockGetTreeArgs,
        currentPackages,
        wantedPackages: currentPackages,
        graph,
        materializationCache,
      }

      // depth 1: should only show 'a' without children
      const shallow = getTree({ ...baseOpts, maxDepth: 1 }, rootNodeId)
      expect(shallow).toHaveLength(1)
      expect(shallow[0].dependencies).toBeUndefined()

      // depth Infinity: should show full tree (not affected by depth-1 cache)
      const deep = getTree({ ...baseOpts, maxDepth: Infinity }, rootNodeId)
      expect(deep).toHaveLength(1)
      expect(deep[0].dependencies).toHaveLength(1) // b
    })
  })

  test('exclude peers', () => {
    const version = '1.0.0'
    const currentPackages = {
      'foo@1.0.0': {
        dependencies: {
          peer1: '1.0.0',
          peer2: '1.0.0',
          qar: '1.0.0',
        },
        peerDependencies: {
          peer1: '^1.0.0',
          peer2: '^1.0.0',
        },
        resolution: { integrity: '000' },
      },
      'bar@1.0.0': {
        resolution: { integrity: '000' },
      },
      'qar@1.0.0': {
        resolution: { integrity: '000' },
      },
      'peer1@1.0.0': {
        dependencies: {
          bar: '1.0.0',
        },
        resolution: { integrity: '000' },
      },
      'peer2@1.0.0': {
        dependencies: {},
        resolution: { integrity: '000' },
      },
    }
    const rootNodeId: TreeNodeId = { type: 'package', depPath: refToRelativeOrThrow(version, 'foo') }

    const getTreeArgs = {
      depTypes: {},
      excludePeerDependencies: true,
      maxDepth: 3,
      rewriteLinkVersionDir: '',
      virtualStoreDir: '.pnpm',
      importers: {},
      include: { optionalDependencies: false },
      lockfileDir: '',
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
      currentPackages,
      wantedPackages: currentPackages,
    }
    const result = normalizeDependencyNodeForTesting(getTreeWithGraph({ ...getTreeArgs, maxDepth: 9999, virtualStoreDirMaxLength: 120 }, rootNodeId))

    expect(result).toEqual([
      expect.objectContaining({
        alias: 'peer1',
        dependencies: [
          expect.objectContaining({
            alias: 'bar',
          }),
        ],
      }),
      expect.objectContaining({ alias: 'qar', dependencies: undefined }),
    ])
  })
})
