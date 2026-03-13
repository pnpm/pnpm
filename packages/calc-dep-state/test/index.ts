import { calcDepState, calcGraphNodeHash } from '@pnpm/calc-dep-state'
import { ENGINE_NAME } from '@pnpm/constants'
import { hashObject, hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'
import type { DepPath, PkgIdWithPatchHash } from '@pnpm/types'

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
