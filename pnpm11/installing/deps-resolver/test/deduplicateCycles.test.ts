import { expect, test } from '@jest/globals'
import type { DepPath, PkgIdWithPatchHash, PkgResolutionId } from '@pnpm/types'

import { deduplicateAll } from '../lib/deduplicateDepPaths.js'
import type { GenericDependenciesGraphWithResolvedChildren, PartialResolvedPackage } from '../lib/peersResolutionTypes.js'

test.each([[2, false, 1], [2, true, 1], [128, false, 1], [2, false, 128]] as const)('dedupe size %s, reverse %s, components %s', (size, reverse, count) => {
  const { graph, groups } = cycleGraph(size, [1], count)
  const replacements = deduplicateAll({ depGraph: graph }, reverse ? groups.reverse() : groups)
  for (let index = 0; index < size * count; index++) {
    expect(replacements[`node-${index}@1.0.0` as DepPath]).toBe(`node-${index}@1.0.0(peer@1.0.0)`)
  }
  expect(graph['unrelated-0@1.0.0' as DepPath].children.leaf).toBe('leaf@1.0.0')
})

test('a parent of a cycle waits for the cyclic variants to settle', () => {
  const { graph, groups } = cycleGraph(2, [1, 2])
  const ancestors = new Set<DepPath>()
  for (const suffix of ['', '(peer@1.0.0)']) {
    const depPath = `ancestor@1.0.0${suffix}` as DepPath
    graph[depPath] = {
      ...graph[`node-0@1.0.0${suffix}` as DepPath],
      depPath,
      pkgIdWithPatchHash: 'ancestor@1.0.0' as PkgIdWithPatchHash,
      children: { child: `node-0@1.0.0${suffix}` as DepPath },
    }
    ancestors.add(depPath)
  }
  groups.unshift(ancestors)
  const replacements = deduplicateAll({ depGraph: graph }, groups)
  const parent = replacements['ancestor@1.0.0' as DepPath] ?? 'ancestor@1.0.0' as DepPath
  const direct = replacements['node-0@1.0.0' as DepPath] ?? 'node-0@1.0.0' as DepPath
  expect(graph[parent].children.child).toBe(direct)
  expect(direct).toBe('node-0@1.0.0(peer@2.0.0)')
})

function cycleGraph (size: number, peerVersions = [1], count = 1) {
  const graph: GenericDependenciesGraphWithResolvedChildren<PartialResolvedPackage> = {}
  const groups: Array<Set<DepPath>> = []
  for (let index = 0; index < size * count; index++) {
    const name = `node-${index}`
    const child = `node-${Math.floor(index / size) * size + (index + 1) % size}`
    const group = new Set<DepPath>()
    for (const peerVersion of [0, ...peerVersions]) {
      const bound = peerVersion > 0
      const suffix = peerSuffix(peerVersion)
      const depPath = `${name}@1.0.0${suffix}` as DepPath
      group.add(depPath)
      graph[depPath] = {
        name,
        version: '1.0.0',
        id: `${name}@1.0.0` as PkgResolutionId,
        pkgIdWithPatchHash: `${name}@1.0.0` as PkgIdWithPatchHash,
        peers: { declared: {}, taken: {} },
        children: cycleChildren(child, peerVersion),
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
  for (const peerVersion of peerVersions) {
    const depPath = `peer@${peerVersion}.0.0` as DepPath
    graph[depPath] = {
      ...graph['node-0@1.0.0' as DepPath],
      depPath,
      pkgIdWithPatchHash: `peer@${peerVersion}.0.0` as PkgIdWithPatchHash,
      children: {},
    }
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

function cycleChildren (child: string, peerVersion: number): Record<string, DepPath> {
  const children: Record<string, DepPath> = {}
  const suffix = peerSuffix(peerVersion)
  children[child] = `${child}@1.0.0${suffix}` as DepPath
  if (peerVersion > 0) children.peer = `peer@${peerVersion}.0.0` as DepPath
  return children
}

function peerSuffix (version: number): string {
  return version > 0 ? `(peer@${version}.0.0)` : ''
}
