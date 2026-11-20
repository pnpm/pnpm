import { expect, test } from '@jest/globals'
import type { DepPath, PkgIdWithPatchHash, PkgResolutionId } from '@pnpm/types'

import { deduplicateAll } from '../lib/deduplicateDepPaths.js'
import type { GenericDependenciesGraphWithResolvedChildren, PartialResolvedPackage } from '../lib/peersResolutionTypes.js'

// https://github.com/pnpm/pnpm/issues/17781
test('a variant that absorbs one that must leave a peer unbound does not later collapse into a variant that resolves it', () => {
  const graph: GenericDependenciesGraphWithResolvedChildren<PartialResolvedPackage> = {}
  const addNode = (depPath: string, children: Record<string, string>, resolvedPeerNames: string[] = []): DepPath => {
    const pkgIdWithPatchHash = depPath.replace(/\(.*$/, '').replace(/#.*$/, '')
    graph[depPath as DepPath] = {
      name: pkgIdWithPatchHash.split('@')[0],
      version: '1.0.0',
      id: pkgIdWithPatchHash as PkgResolutionId,
      pkgIdWithPatchHash: pkgIdWithPatchHash as PkgIdWithPatchHash,
      peers: { declared: {}, taken: {} },
      children: children as Record<string, DepPath>,
      resolvedPeerNames: new Set(resolvedPeerNames),
      transitivePeerDependencies: new Set(),
      depth: 0,
      installable: true,
      isPure: false,
      dir: '',
      modules: '',
      depPath: depPath as DepPath,
    }
    return depPath as DepPath
  }
  addNode('p@1.0.0', {})
  addNode('q@1.0.0', {})
  addNode('e@1.0.0', {})
  const x0 = addNode('x@1.0.0', {})
  const x1 = addNode('x@1.0.0(q@1.0.0)', { q: 'q@1.0.0' }, ['q'])
  const protectedVariant = addNode('a@1.0.0', { x: x0 })
  const unprotected = addNode('a@1.0.0#extra', { x: x0, e: 'e@1.0.0' })
  const resolvesPeer = addNode('a@1.0.0(p@1.0.0)', { x: x1, e: 'e@1.0.0', p: 'p@1.0.0' }, ['p'])

  const replacements = deduplicateAll({
    depGraph: graph,
    unboundOptionalPeers: new Map([[protectedVariant, new Set(['p'])]]),
  }, [new Set([protectedVariant, unprotected, resolvesPeer]), new Set([x0, x1])])

  expect(replacements[protectedVariant]).toBe(unprotected)
  expect(replacements[unprotected]).toBeUndefined()
})
