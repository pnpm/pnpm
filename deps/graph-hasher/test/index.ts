import { describe, expect, test } from '@jest/globals'
import { hashObject, hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import { calcDepState, calcGraphNodeHash, findRuntimeNodeVersion, readSnapshotRuntimePin } from '@pnpm/deps.graph-hasher'
import { engineName } from '@pnpm/engine.runtime.system-version'
import type { DepPath, PkgIdWithPatchHash } from '@pnpm/types'

// Match the function the production code uses (see
// `deps/graph-hasher/src/index.ts`). In non-SEA test contexts this
// equals `process.version`-derived ENGINE_NAME, so existing assertions
// keep working; in SEA contexts it tracks the script-runner Node.
const ENGINE_NAME = engineName()

const depsGraph = {
  'foo@1.0.0': {
    pkgIdWithPatchHash: 'foo@1.0.0' as PkgIdWithPatchHash,
    resolution: {
      integrity: '000',
    },
    children: {
      bar: 'bar@1.0.0',
    },
  },
  'bar@1.0.0': {
    pkgIdWithPatchHash: 'bar@1.0.0' as PkgIdWithPatchHash,
    resolution: {
      integrity: '001',
    },
    children: {
      foo: 'foo@1.0.0',
    },
  },
}

test('calcDepState()', () => {
  expect(calcDepState(depsGraph, {}, 'foo@1.0.0', {
    includeDepGraphHash: true,
  })).toBe(`${ENGINE_NAME};deps=${hashObject({
    id: 'foo@1.0.0:000',
    deps: {
      bar: hashObject({
        id: 'bar@1.0.0:001',
        deps: {
          foo: hashObject({
            id: 'foo@1.0.0:000',
            deps: {},
          }),
        },
      }),
    },
  })}`)
})

test('calcDepState() when scripts are ignored', () => {
  expect(calcDepState(depsGraph, {}, 'foo@1.0.0', {
    includeDepGraphHash: false,
  })).toBe(ENGINE_NAME)
})

test('findRuntimeNodeVersion() pulls the pinned major from a node@runtime: snapshot key', () => {
  // Mirrors pacquet's `find_runtime_node_major` helper; both must
  // agree on the version-extraction rule or the two tools would
  // hash GVS slots under different engine majors for the same
  // project. The peer-suffixed form must reduce to the same bare
  // version as the form without a peer suffix.
  expect(
    findRuntimeNodeVersion(['leftpad@1.3.0', 'node@runtime:22.11.0'])
  ).toBe('22.11.0')
  expect(
    findRuntimeNodeVersion(['node@runtime:22.11.0(node@22.11.0)'])
  ).toBe('22.11.0')
  expect(
    findRuntimeNodeVersion(['leftpad@1.3.0', 'is-positive@3.1.0'])
  ).toBeUndefined()
})

test('readSnapshotRuntimePin() pulls the own pin from a graph node child', () => {
  // The resolver desugars a dep's `engines.runtime` into
  // `dependencies.node: 'runtime:<version>'` and `refToRelative`
  // encodes that into the `node@runtime:<version>[(peers)]` DepPath
  // the graph carries as `children.node`. The per-snapshot lookup
  // reads back the bare version from there. Without this branch
  // the GVS hash for the pinning snapshot would key under the
  // install-wide Node, not the Node the bin linker spawns for it.
  expect(readSnapshotRuntimePin({ node: 'node@runtime:22.11.0' })).toBe('22.11.0')
  expect(readSnapshotRuntimePin({ node: 'node@runtime:22.11.0(node@22.11.0)' })).toBe('22.11.0')
  expect(readSnapshotRuntimePin({ node: 'node@22.11.0' })).toBeUndefined()
  expect(readSnapshotRuntimePin({ leftpad: 'leftpad@1.3.0' })).toBeUndefined()
  expect(readSnapshotRuntimePin({})).toBeUndefined()
  expect(readSnapshotRuntimePin(undefined)).toBeUndefined()
})

test('calcDepState() uses the snapshot\'s own engines.runtime pin', () => {
  // A package whose graph node has `children.node = node@runtime:...`
  // pinned its own Node via `engines.runtime`; the side-effects-cache
  // key prefix has to encode *that* major (not the install-wide
  // fallback) because the bin linker spawns lifecycle scripts on the
  // package's pinned Node, not the install-wide one.
  const graph = {
    'pinned@1.0.0': {
      pkgIdWithPatchHash: 'pinned@1.0.0' as PkgIdWithPatchHash,
      resolution: { integrity: '900' },
      children: { node: 'node@runtime:22.11.0' },
    },
    'node@runtime:22.11.0': {
      pkgIdWithPatchHash: 'node@runtime:22.11.0' as PkgIdWithPatchHash,
      resolution: { integrity: '901' },
      children: {},
    },
  }
  expect(calcDepState(graph, {}, 'pinned@1.0.0', {
    includeDepGraphHash: false,
    nodeVersion: '20.5.0', // install-wide fallback differs from own pin
  })).toBe(`${process.platform};${process.arch};node22`)
})

describe('calcGraphNodeHash', () => {
  const graphNodeGraph = {
    'foo@1.0.0': {
      children: { bar: 'bar@1.0.0' as DepPath },
      pkgIdWithPatchHash: 'foo@1.0.0' as PkgIdWithPatchHash,
      resolution: { integrity: '000' },
    },
    'bar@1.0.0': {
      children: {},
      pkgIdWithPatchHash: 'bar@1.0.0' as PkgIdWithPatchHash,
      resolution: { integrity: '001' },
    },
    'native@1.0.0': {
      children: {},
      pkgIdWithPatchHash: 'native@1.0.0' as PkgIdWithPatchHash,
      resolution: { integrity: '002' },
    },
    'depends-on-native@1.0.0': {
      children: { native: 'native@1.0.0' as DepPath },
      pkgIdWithPatchHash: 'depends-on-native@1.0.0' as PkgIdWithPatchHash,
      resolution: { integrity: '003' },
    },
  } as Record<DepPath, { children: Record<string, DepPath>, pkgIdWithPatchHash: PkgIdWithPatchHash, resolution: { integrity: string } }>

  test('includes ENGINE_NAME when builtDepPaths is not provided', () => {
    const hash = calcGraphNodeHash(
      { graph: graphNodeGraph, cache: {} },
      { depPath: 'foo@1.0.0' as DepPath, name: 'foo', version: '1.0.0' }
    )
    expect(hash).toContain('foo/1.0.0/')
    // Hash should include ENGINE_NAME (default behavior)
    const depsHash = hashObject({
      id: 'foo@1.0.0:000',
      deps: {
        bar: hashObject({ id: 'bar@1.0.0:001', deps: {} }),
      },
    })
    const expectedDigest = hashObjectWithoutSorting(
      { engine: ENGINE_NAME, deps: depsHash },
      { encoding: 'hex' }
    )
    expect(hash).toBe(`@/foo/1.0.0/${expectedDigest}`)
  })

  test('omits ENGINE_NAME for pure-JS packages when builtDepPaths is provided', () => {
    const builtDepPaths = new Set<DepPath>(['native@1.0.0' as DepPath])
    const hash = calcGraphNodeHash(
      { graph: graphNodeGraph, cache: {}, builtDepPaths, buildRequiredCache: {} },
      { depPath: 'foo@1.0.0' as DepPath, name: 'foo', version: '1.0.0' }
    )
    const depsHash = hashObject({
      id: 'foo@1.0.0:000',
      deps: {
        bar: hashObject({ id: 'bar@1.0.0:001', deps: {} }),
      },
    })
    const expectedDigest = hashObjectWithoutSorting(
      { engine: null, deps: depsHash },
      { encoding: 'hex' }
    )
    expect(hash).toBe(`@/foo/1.0.0/${expectedDigest}`)
  })

  test('includes ENGINE_NAME for packages that require a build', () => {
    const builtDepPaths = new Set<DepPath>(['native@1.0.0' as DepPath])
    const hash = calcGraphNodeHash(
      { graph: graphNodeGraph, cache: {}, builtDepPaths, buildRequiredCache: {} },
      { depPath: 'native@1.0.0' as DepPath, name: 'native', version: '1.0.0' }
    )
    const depsHash = hashObject({ id: 'native@1.0.0:002', deps: {} })
    const expectedDigest = hashObjectWithoutSorting(
      { engine: ENGINE_NAME, deps: depsHash },
      { encoding: 'hex' }
    )
    expect(hash).toBe(`@/native/1.0.0/${expectedDigest}`)
  })

  test('includes ENGINE_NAME for packages that transitively depend on a built package', () => {
    const builtDepPaths = new Set<DepPath>(['native@1.0.0' as DepPath])
    const hash = calcGraphNodeHash(
      { graph: graphNodeGraph, cache: {}, builtDepPaths, buildRequiredCache: {} },
      { depPath: 'depends-on-native@1.0.0' as DepPath, name: 'depends-on-native', version: '1.0.0' }
    )
    const depsHash = hashObject({
      id: 'depends-on-native@1.0.0:003',
      deps: {
        native: hashObject({ id: 'native@1.0.0:002', deps: {} }),
      },
    })
    const expectedDigest = hashObjectWithoutSorting(
      { engine: ENGINE_NAME, deps: depsHash },
      { encoding: 'hex' }
    )
    expect(hash).toBe(`@/depends-on-native/1.0.0/${expectedDigest}`)
  })

  test('omits ENGINE_NAME when builtDepPaths is empty', () => {
    const builtDepPaths = new Set<DepPath>()
    const hash = calcGraphNodeHash(
      { graph: graphNodeGraph, cache: {}, builtDepPaths, buildRequiredCache: {} },
      { depPath: 'foo@1.0.0' as DepPath, name: 'foo', version: '1.0.0' }
    )
    const depsHash = hashObject({
      id: 'foo@1.0.0:000',
      deps: {
        bar: hashObject({ id: 'bar@1.0.0:001', deps: {} }),
      },
    })
    const expectedDigest = hashObjectWithoutSorting(
      { engine: null, deps: depsHash },
      { encoding: 'hex' }
    )
    expect(hash).toBe(`@/foo/1.0.0/${expectedDigest}`)
  })
})
