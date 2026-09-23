import { expect, test } from '@jest/globals'
import type { DepPath, PkgIdWithPatchHash, PkgResolutionId, ProjectRootDir } from '@pnpm/types'

import { isCompatibleAndHasMoreDeps } from '../lib/depPathCompatibility.js'
import type { NodeId } from '../lib/nextNodeId.js'
import type { DependenciesTreeNode } from '../lib/resolveDependencies.js'
import { type PartialResolvedPackage, resolvePeers } from '../lib/resolvePeers.js'

test('packages are not deduplicated when versions do not match', async () => {
  const fooPkg: PartialResolvedPackage = {
    name: 'foo',
    version: '1.0.0',
    pkgIdWithPatchHash: 'foo/1.0.0' as PkgIdWithPatchHash,
    id: '' as PkgResolutionId,
    peerDependencies: {
      bar: { version: '1.0.0 || 2.0.0' },
      baz: { version: '1.0.0 || 2.0.0', optional: true },
    },
  }

  const peers: Record<string, PartialResolvedPackage> = Object.fromEntries(
    [
      ['bar', '1.0.0'],
      ['bar', '2.0.0'],
      ['baz', '1.0.0'],
      ['baz', '2.0.0'],
    ].map(([name, version]) => [
      `${name}_${version.replace(/\./g, '_')}`,
      {
        name,
        version,
        pkgIdWithPatchHash: `${name}/${version}` as PkgIdWithPatchHash,
        peerDependencies: {},
        id: '' as PkgResolutionId,
      } satisfies PartialResolvedPackage,
    ])
  )

  const { dependenciesByProjectId } = await resolvePeers({
    allPeerDepNames: new Set(['bar', 'baz']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['foo', '>project1>foo/1.0.0>' as NodeId],
          ['bar', '>project1>bar/1.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: 'project1' as PkgResolutionId,
      },
      {
        directNodeIdsByAlias: new Map([
          ['foo', '>project2>foo/1.0.0>' as NodeId],
          ['bar', '>project2>bar/1.0.0>' as NodeId],
          ['baz', '>project2>baz/1.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: 'project2' as PkgResolutionId,
      },
      {
        directNodeIdsByAlias: new Map([
          ['foo', '>project3>foo/1.0.0>' as NodeId],
          ['bar', '>project3>bar/2.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: 'project3' as PkgResolutionId,
      },
      {
        directNodeIdsByAlias: new Map([
          ['foo', '>project4>foo/1.0.0>' as NodeId],
          ['bar', '>project4>bar/2.0.0>' as NodeId],
          ['baz', '>project4>baz/2.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: 'project4' as PkgResolutionId,
      },
    ],
    resolvedImporters: {},
    dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>(([
      ['>project1>foo/1.0.0>' as NodeId, fooPkg],
      ['>project1>bar/1.0.0>' as NodeId, peers.bar_1_0_0],

      ['>project2>foo/1.0.0>' as NodeId, fooPkg],
      ['>project2>bar/1.0.0>' as NodeId, peers.bar_1_0_0],
      ['>project2>baz/1.0.0>' as NodeId, peers.baz_1_0_0],

      ['>project3>foo/1.0.0>' as NodeId, fooPkg],
      ['>project3>bar/2.0.0>' as NodeId, peers.bar_2_0_0],

      ['>project4>foo/1.0.0>' as NodeId, fooPkg],
      ['>project4>bar/2.0.0>' as NodeId, peers.bar_2_0_0],
      ['>project4>baz/2.0.0>' as NodeId, peers.baz_2_0_0],

    ] satisfies Array<[NodeId, PartialResolvedPackage]>).map(([path, resolvedPackage]) => [path, {
      children: {},
      installable: {},
      resolvedPackage,
      depth: 0,
    } as DependenciesTreeNode<PartialResolvedPackage>])),
    dedupePeerDependents: true,
    virtualStoreDir: '',
    virtualStoreDirMaxLength: 120,
    lockfileDir: '',
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })

  expect(dependenciesByProjectId.project1.get('foo')).toEqual(dependenciesByProjectId.project2.get('foo'))
  expect(dependenciesByProjectId.project1.get('foo')).not.toEqual(dependenciesByProjectId.project3.get('foo'))
  expect(dependenciesByProjectId.project3.get('foo')).toEqual(dependenciesByProjectId.project4.get('foo'))
})

// When a peer-suffixed variant is a subset of two mutually incompatible larger
// variants, the dedupe pass has to pick which one to collapse it into. That
// choice must not depend on the order the importers happen to be processed in —
// otherwise the same workspace resolves to different lockfiles on different
// machines, and `pnpm dedupe --check` flips between pass and fail.
test('peer-dependent deduplication does not depend on importer order', async () => {
  const fooPkg: PartialResolvedPackage = {
    name: 'foo',
    version: '1.0.0',
    pkgIdWithPatchHash: 'foo/1.0.0' as PkgIdWithPatchHash,
    id: '' as PkgResolutionId,
    peerDependencies: {
      bar: { version: '1.0.0', optional: true },
      baz: { version: '1.0.0', optional: true },
      qux: { version: '1.0.0', optional: true },
    },
  }
  const purePeer = (name: string): PartialResolvedPackage => ({
    name,
    version: '1.0.0',
    pkgIdWithPatchHash: `${name}/1.0.0` as PkgIdWithPatchHash,
    peerDependencies: {},
    id: '' as PkgResolutionId,
  })

  // project-subset resolves foo(bar); project-baz resolves foo(bar)(baz);
  // project-qux resolves foo(bar)(qux). foo(bar) is a subset of both of the
  // larger variants, which are themselves incompatible with each other.
  const makeProject = (id: string, aliases: string[]) => ({
    directNodeIdsByAlias: new Map(aliases.map((alias) => [alias, `>${id}>${alias}>` as NodeId])),
    topParents: [],
    rootDir: '' as ProjectRootDir,
    id: id as PkgResolutionId,
  })
  const projectSubset = makeProject('project-subset', ['foo', 'bar'])
  const projectBaz = makeProject('project-baz', ['foo', 'bar', 'baz'])
  const projectQux = makeProject('project-qux', ['foo', 'bar', 'qux'])

  const buildTree = () => new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>(([
    ['>project-subset>foo>' as NodeId, fooPkg],
    ['>project-subset>bar>' as NodeId, purePeer('bar')],
    ['>project-baz>foo>' as NodeId, fooPkg],
    ['>project-baz>bar>' as NodeId, purePeer('bar')],
    ['>project-baz>baz>' as NodeId, purePeer('baz')],
    ['>project-qux>foo>' as NodeId, fooPkg],
    ['>project-qux>bar>' as NodeId, purePeer('bar')],
    ['>project-qux>qux>' as NodeId, purePeer('qux')],
  ] satisfies Array<[NodeId, PartialResolvedPackage]>).map(([path, resolvedPackage]) => [path, {
    children: {},
    installable: {},
    resolvedPackage,
    depth: 0,
  } as DependenciesTreeNode<PartialResolvedPackage>]))

  const resolveSubsetFoo = async (projects: Array<ReturnType<typeof makeProject>>) => {
    const { dependenciesByProjectId } = await resolvePeers({
      allPeerDepNames: new Set(['bar', 'baz', 'qux']),
      projects,
      resolvedImporters: {},
      dependenciesTree: buildTree(),
      dedupePeerDependents: true,
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })
    return dependenciesByProjectId['project-subset'].get('foo')
  }

  const bazFirst = await resolveSubsetFoo([projectSubset, projectBaz, projectQux])
  const quxFirst = await resolveSubsetFoo([projectSubset, projectQux, projectBaz])

  expect(bazFirst).toBeDefined()
  expect(quxFirst).toBeDefined()
  expect(bazFirst).toBe(quxFirst)
})

// The three `child` variants cannot all collapse in one round: `child(other)`
// absorbs neither `child(optPeer)` nor the other way round, so the child group
// still holds a leftover when the round ends and the graph's child edges are
// never rewritten. The parents must therefore collapse on the strength of their
// children being compatible variants of one package, not on their child
// depPaths being equal. See https://github.com/pnpm/pnpm/issues/14800
test('a package whose child carries an optional peer suffix absorbs the variant whose child does not', async () => {
  const childPkg: PartialResolvedPackage = {
    name: 'child',
    version: '1.0.0',
    pkgIdWithPatchHash: 'child/1.0.0' as PkgIdWithPatchHash,
    id: '' as PkgResolutionId,
    peerDependencies: {
      optPeer: { version: '1.0.0', optional: true },
      other: { version: '1.0.0', optional: true },
    },
  }

  const parentPkg: PartialResolvedPackage = {
    name: 'parent',
    version: '1.0.0',
    pkgIdWithPatchHash: 'parent/1.0.0' as PkgIdWithPatchHash,
    id: '' as PkgResolutionId,
    peerDependencies: {},
  }

  const peerPkg = (name: string): PartialResolvedPackage => ({
    name,
    version: '1.0.0',
    pkgIdWithPatchHash: `${name}/1.0.0` as PkgIdWithPatchHash,
    id: '' as PkgResolutionId,
    peerDependencies: {},
  })

  const treeNode = (resolvedPackage: PartialResolvedPackage, children: Record<string, NodeId> = {}) => ({
    children,
    installable: true,
    resolvedPackage,
    depth: 0,
  } as DependenciesTreeNode<PartialResolvedPackage>)

  const projectIds = ['projectOptPeer', 'projectOther', 'projectBare'] as const
  const parentNodeId = (projectId: string) => `>${projectId}>parent/1.0.0>` as NodeId
  const childNodeId = (projectId: string) => `>${projectId}>parent/1.0.0>child/1.0.0>` as NodeId
  const peerNodeId = (projectId: string, name: string) => `>${projectId}>${name}/1.0.0>` as NodeId

  const dependenciesTree = new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>()
  for (const projectId of projectIds) {
    dependenciesTree.set(parentNodeId(projectId), treeNode(parentPkg, { child: childNodeId(projectId) }))
    dependenciesTree.set(childNodeId(projectId), treeNode(childPkg))
  }
  dependenciesTree.set(peerNodeId('projectOptPeer', 'optPeer'), treeNode(peerPkg('optPeer')))
  dependenciesTree.set(peerNodeId('projectOther', 'other'), treeNode(peerPkg('other')))

  const { dependenciesByProjectId } = await resolvePeers({
    allPeerDepNames: new Set(['optPeer', 'other']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['parent', parentNodeId('projectOptPeer')],
          ['optPeer', peerNodeId('projectOptPeer', 'optPeer')],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: 'projectOptPeer' as PkgResolutionId,
      },
      {
        directNodeIdsByAlias: new Map([
          ['parent', parentNodeId('projectOther')],
          ['other', peerNodeId('projectOther', 'other')],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: 'projectOther' as PkgResolutionId,
      },
      {
        directNodeIdsByAlias: new Map([
          ['parent', parentNodeId('projectBare')],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: 'projectBare' as PkgResolutionId,
      },
    ],
    resolvedImporters: {},
    dependenciesTree,
    dedupePeerDependents: true,
    virtualStoreDir: '',
    virtualStoreDirMaxLength: 120,
    lockfileDir: '',
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })

  expect(dependenciesByProjectId.projectOptPeer.get('parent')).toBe('parent/1.0.0(optPeer/1.0.0)')
  expect(dependenciesByProjectId.projectOther.get('parent')).toBe('parent/1.0.0(other/1.0.0)')
  expect(dependenciesByProjectId.projectBare.get('parent')).toBe('parent/1.0.0(other/1.0.0)')
})

// Chain longer than the call stack's budget, diverging at every level so the
// compatibility walk has to reach the bottom. Compatibility stays answerable at
// a depth the call stack cannot hold.
test('a deep chain of peer-suffixed children does not overflow the call stack', () => {
  const depth = 30_000
  const depGraph: Record<string, unknown> = {}
  for (let level = 0; level < depth; level++) {
    const last = level + 1 === depth
    const pkgIdWithPatchHash = `pkg${level}/1.0.0`
    depGraph[`pkg${level}/1.0.0(peer/1.0.0)`] = {
      pkgIdWithPatchHash,
      children: last ? {} : { next: `pkg${level + 1}/1.0.0(peer/1.0.0)` },
      resolvedPeerNames: new Set(['peer']),
    }
    depGraph[pkgIdWithPatchHash] = {
      pkgIdWithPatchHash,
      children: last ? {} : { next: `pkg${level + 1}/1.0.0` },
      resolvedPeerNames: new Set(),
    }
  }

  expect(isCompatibleAndHasMoreDeps(
    depGraph as Parameters<typeof isCompatibleAndHasMoreDeps>[0],
    'pkg0/1.0.0(peer/1.0.0)' as DepPath,
    'pkg0/1.0.0' as DepPath
  )).toBe(true)
})

// Covers https://github.com/pnpm/pnpm/issues/6200
test('dependencies with peer dependencies do not resolve to peer versions from another workspace project when dedupePeerDependents is true', async () => {
  const hostPkg: PartialResolvedPackage = {
    name: 'host',
    version: '1.0.0',
    pkgIdWithPatchHash: 'host/1.0.0' as PkgIdWithPatchHash,
    id: '' as PkgResolutionId,
    peerDependencies: {
      peer: { version: '>=1.0.0' },
    },
  }

  const dependentPkg: PartialResolvedPackage = {
    name: 'dependent',
    version: '1.0.0',
    pkgIdWithPatchHash: 'dependent/1.0.0' as PkgIdWithPatchHash,
    id: '' as PkgResolutionId,
    peerDependencies: {
      host: { version: '1.0.0' },
      peer: { version: '>=1.0.0' },
    },
  }

  const peerPkg = (version: string): PartialResolvedPackage => ({
    name: 'peer',
    version,
    pkgIdWithPatchHash: `peer/${version}` as PkgIdWithPatchHash,
    peerDependencies: {},
    id: '' as PkgResolutionId,
  })

  const treeNode = (resolvedPackage: PartialResolvedPackage, children: Record<string, NodeId> = {}) => ({
    children,
    installable: true,
    resolvedPackage,
    depth: 0,
  } as DependenciesTreeNode<PartialResolvedPackage>)

  const dependenciesTree = new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
    ['>project1>dependent/1.0.0>' as NodeId, treeNode(dependentPkg)],
    ['>project1>host/1.0.0>' as NodeId, treeNode(hostPkg)],
    ['>project1>peer/1.0.0>' as NodeId, treeNode(peerPkg('1.0.0'))],

    ['>project2>dependent/1.0.0>' as NodeId, treeNode(dependentPkg)],
    ['>project2>host/1.0.0>' as NodeId, treeNode(hostPkg)],
    ['>project2>peer/2.0.0>' as NodeId, treeNode(peerPkg('2.0.0'))],
  ])

  const { dependenciesByProjectId } = await resolvePeers({
    allPeerDepNames: new Set(['host', 'peer']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['dependent', '>project1>dependent/1.0.0>' as NodeId],
          ['host', '>project1>host/1.0.0>' as NodeId],
          ['peer', '>project1>peer/1.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: 'project1' as PkgResolutionId,
      },
      {
        directNodeIdsByAlias: new Map([
          ['dependent', '>project2>dependent/1.0.0>' as NodeId],
          ['host', '>project2>host/1.0.0>' as NodeId],
          ['peer', '>project2>peer/2.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: 'project2' as PkgResolutionId,
      },
    ],
    resolvedImporters: {},
    dependenciesTree,
    dedupePeerDependents: true,
    virtualStoreDir: '',
    virtualStoreDirMaxLength: 120,
    lockfileDir: '',
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })

  expect(dependenciesByProjectId.project1.get('host')).toBe('host/1.0.0(peer/1.0.0)')
  expect(dependenciesByProjectId.project2.get('host')).toBe('host/1.0.0(peer/2.0.0)')
  expect(dependenciesByProjectId.project1.get('dependent')).toBe('dependent/1.0.0(host/1.0.0(peer/1.0.0))(peer/1.0.0)')
  expect(dependenciesByProjectId.project2.get('dependent')).toBe('dependent/1.0.0(host/1.0.0(peer/2.0.0))(peer/2.0.0)')
})

