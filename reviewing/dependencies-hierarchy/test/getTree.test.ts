import { refToRelative } from '@pnpm/dependency-path'
import { PackageSnapshots } from '@pnpm/lockfile-file'
import { PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'
import { getTree } from '../lib/getTree'

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
    const startingDepPath = refToRelativeOrThrow(version, 'a')

    const getTreeArgs = {
      maxDepth: 0,
      modulesDir: '',
      includeOptionalDependencies: false,
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
      currentPackages,
      wantedPackages: currentPackages,
    }

    test('full test case to print when max depth is large', () => {
      const result = normalizePackageNodeForTesting(getTree({ ...getTreeArgs, currentDepth: 0, maxDepth: 9999 }, [], startingDepPath))

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
      const result = getTree({ ...getTreeArgs, currentDepth: 1, maxDepth: 0 }, [], startingDepPath)
      expect(result).toEqual([])
    })

    test('max depth of 0 to print flat dependencies', () => {
      const result = getTree({ ...getTreeArgs, currentDepth: 0, maxDepth: 0 }, [], startingDepPath)

      expect(normalizePackageNodeForTesting(result)).toEqual([
        expect.objectContaining({ alias: 'b1', dependencies: undefined }),
        expect.objectContaining({ alias: 'b2', dependencies: undefined }),
        expect.objectContaining({ alias: 'b3', dependencies: undefined }),
      ])
    })

    test('max depth of 1 to print a1 -> b1 -> c1, but not d1', () => {
      const result = getTree({ ...getTreeArgs, currentDepth: 0, maxDepth: 1 }, [], startingDepPath)

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
      modulesDir: '',
      includeOptionalDependencies: false,
      skipped: new Set<string>(),
      registries: {
        default: 'mock-registry-for-testing.example',
      },
    }

    test('revisiting package at lower depth prints dependenices not previously printed', () => {
      // This tests the "glob" npm package on a subset of its dependency tree.
      // Requested depth (max depth - current depth) shown in square brackets.
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
      const rootDepPath = refToRelativeOrThrow(version, 'root')

      const result = getTree({
        ...commonMockGetTreeArgs,
        currentDepth: 0,
        maxDepth: 2,
        currentPackages,
        wantedPackages: currentPackages,
      }, [rootDepPath], rootDepPath)

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

    test('revisiting package at higher depth does not print extra dependenices', () => {
      // This tests the "glob" npm package on a subset of its dependency tree.
      // Requested depth (max depth - current depth) shown in square brackets.
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
      const rootDepPath = refToRelativeOrThrow(version, 'root')

      const result = getTree({
        ...commonMockGetTreeArgs,
        currentDepth: 0,
        maxDepth: 2,
        currentPackages,
        wantedPackages: currentPackages,
      }, [rootDepPath], rootDepPath)

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
})
