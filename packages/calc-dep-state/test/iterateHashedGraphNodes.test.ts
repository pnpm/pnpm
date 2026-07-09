import { iterateHashedGraphNodes, type DepsGraph, type PkgMeta } from '@pnpm/calc-dep-state'
import { type DepPath, type PkgIdWithPatchHash } from '@pnpm/types'

function hashGraphNode (version: string): IteratorResult<{ hash: string }> {
  const depPath = 'foo@1.0.0' as DepPath
  const graph: DepsGraph<DepPath> = {
    [depPath]: {
      pkgIdWithPatchHash: 'foo@1.0.0' as PkgIdWithPatchHash,
      resolution: { integrity: 'sha512-deadbeef' },
      children: {},
    },
  }
  function * pkgMetaIterator (): IterableIterator<PkgMeta> {
    yield { name: 'foo', version, depPath }
  }
  return iterateHashedGraphNodes(graph, pkgMetaIterator())[Symbol.iterator]().next()
}

test('iterateHashedGraphNodes builds a global-virtual-store slot path', () => {
  const { value } = hashGraphNode('1.0.0')
  expect(value.hash).toMatch(/^@\/foo\/1\.0\.0\/[a-f0-9]+$/)
})

// The version segment is lockfile-controlled and inserted raw into the slot
// path; a `..` segment would let the slot escape the global virtual store
// root once joined onto the global virtual store dir. All GVS slot builders
// funnel through `iterateHashedGraphNodes`, so the rejection happens here.
test.each(['../../../escape', '..', 'a/../../b', 'x/..'])(
  'iterateHashedGraphNodes rejects a traversal version segment %p',
  (version) => {
    expect(() => hashGraphNode(version))
      .toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
  }
)
