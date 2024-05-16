import { type PartialResolvedPackage, resolvePeers } from '../lib/resolvePeers'
import { type DependenciesTreeNode } from '../lib/resolveDependencies'
import { type NodeId } from '../lib/nextNodeId'

test('packages are not deduplicated when versions do not match', async () => {
  const fooPkg: PartialResolvedPackage = {
    name: 'foo',
    version: '1.0.0',
    depPath: 'foo/1.0.0',
    id: '',
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
        depPath: `${name}/${version}`,
        peerDependencies: {},
        id: '',
      } satisfies PartialResolvedPackage,
    ])
  )

  const { dependenciesByProjectId } = await resolvePeers({
    allPeerDepNames: new Set(['bar', 'baz']),
    projects: [
      {
        directNodeIdsByAlias: {
          foo: '>project1>foo/1.0.0>' as NodeId,
          bar: '>project1>bar/1.0.0>' as NodeId,
        },
        topParents: [],
        rootDir: '',
        id: 'project1',
      },
      {
        directNodeIdsByAlias: {
          foo: '>project2>foo/1.0.0>' as NodeId,
          bar: '>project2>bar/1.0.0>' as NodeId,
          baz: '>project2>baz/1.0.0>' as NodeId,
        },
        topParents: [],
        rootDir: '',
        id: 'project2',
      },
      {
        directNodeIdsByAlias: {
          foo: '>project3>foo/1.0.0>' as NodeId,
          bar: '>project3>bar/2.0.0>' as NodeId,
        },
        topParents: [],
        rootDir: '',
        id: 'project3',
      },
      {
        directNodeIdsByAlias: {
          foo: '>project4>foo/1.0.0>' as NodeId,
          bar: '>project4>bar/2.0.0>' as NodeId,
          baz: '>project4>baz/2.0.0>' as NodeId,
        },
        topParents: [],
        rootDir: '',
        id: 'project4',
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

    ] satisfies Array<[string, PartialResolvedPackage]>).map(([path, resolvedPackage]) => [path, {
      children: {},
      installable: {},
      resolvedPackage,
      depth: 0,
    } as DependenciesTreeNode<PartialResolvedPackage>])),
    dedupePeerDependents: true,
    virtualStoreDir: '',
    virtualStoreDirMaxLength: 120,
    lockfileDir: '',
  })

  expect(dependenciesByProjectId.project1.foo).toEqual(dependenciesByProjectId.project2.foo)
  expect(dependenciesByProjectId.project1.foo).not.toEqual(dependenciesByProjectId.project3.foo)
  expect(dependenciesByProjectId.project3.foo).toEqual(dependenciesByProjectId.project4.foo)
})
