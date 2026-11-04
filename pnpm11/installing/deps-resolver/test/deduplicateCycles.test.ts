import { expect, test } from '@jest/globals'
import type { DepPath, PkgIdWithPatchHash, PkgResolutionId } from '@pnpm/types'

import { deduplicateAll } from '../lib/deduplicateDepPaths.js'
import type { GenericDependenciesGraphWithResolvedChildren, PartialResolvedPackage } from '../lib/peersResolutionTypes.js'

test.each([[2, false], [2, true], [128, false]] as const)('dedupe a cycle of %s with reversed order %s', (size, reverse) => {
  const { graph, groups } = cycleGraph(size)
  const replacements = deduplicateAll(graph, reverse ? groups.reverse() : groups)
  for (let index = 0; index < size; index++) {
    expect(replacements[`node-${index}@1.0.0` as DepPath]).toBe(`node-${index}@1.0.0(peer@1.0.0)`)
  }
  expect(graph['unrelated-0@1.0.0' as DepPath].children.leaf).toBe('leaf@1.0.0')
})

function cycleGraph (size: number) {
  const graph: GenericDependenciesGraphWithResolvedChildren<PartialResolvedPackage> = {}
  const groups: Array<Set<DepPath>> = []
  for (let index = 0; index < size; index++) {
    const name = `node-${index}`
    const child = `node-${(index + 1) % size}`
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
  for (let index = 0; index < 2000; index++) {
    const depPath = `unrelated-${index}@1.0.0` as DepPath
    graph[depPath] = {
      ...graph['node-0@1.0.0' as DepPath],
      depPath,
      pkgIdWithPatchHash: depPath as unknown as PkgIdWithPatchHash,
      children: { leaf: 'leaf@1.0.0' as DepPath },
    }
  }
  return { graph, groups }
}
