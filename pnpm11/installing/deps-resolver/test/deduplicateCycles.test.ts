import { expect, test } from '@jest/globals'
import type { DepPath, PkgIdWithPatchHash, PkgResolutionId } from '@pnpm/types'

import { deduplicateAll } from '../lib/deduplicateDepPaths.js'
import type { GenericDependenciesGraphWithResolvedChildren, PartialResolvedPackage } from '../lib/peersResolutionTypes.js'

test.each([false, true])('a cycle dedupes with reversed group order: %s', (reverse) => {
  const graph: GenericDependenciesGraphWithResolvedChildren<PartialResolvedPackage> = {}
  const groups: Array<Set<DepPath>> = []
  for (const [name, child] of [['alpha', 'beta'], ['beta', 'alpha']]) {
    const group = new Set<DepPath>()
    for (const bound of [false, true]) {
      const suffix = bound ? '(peer@1.0.0)' : ''
      const depPath = `${name}@1.0.0${suffix}` as DepPath
      group.add(depPath)
      graph[depPath] = {
        name,
        version: '1.0.0',
        id: `${name}@1.0.0` as PkgResolutionId,
        pkgIdWithPatchHash: `${name}@1.0.0` as PkgIdWithPatchHash,
        peers: { declared: {}, taken: {} },
        children: { [child]: `${child}@1.0.0${suffix}` as DepPath },
        resolvedPeerNames: new Set(bound ? ['peer'] : []),
        transitivePeerDependencies: new Set(),
        depth: 0,
        installable: true,
        isPure: false,
        dir: '',
        modules: '',
        depPath,
      }
    }
    groups.push(group)
  }
  const replacements = deduplicateAll(graph, reverse ? groups.reverse() : groups)
  expect(replacements['alpha@1.0.0' as DepPath]).toBe('alpha@1.0.0(peer@1.0.0)')
  expect(replacements['beta@1.0.0' as DepPath]).toBe('beta@1.0.0(peer@1.0.0)')
})
