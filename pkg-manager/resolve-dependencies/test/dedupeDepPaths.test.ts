import { type PkgResolutionId, type PkgIdWithPatchHash, type ProjectRootDir } from '@pnpm/types'
import { type PartialResolvedPackage, resolvePeers } from '../lib/resolvePeers'
import { type DependenciesTreeNode } from '../lib/resolveDependencies'
import { type NodeId } from '../lib/nextNodeId'

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
  })

  expect(dependenciesByProjectId.project1.get('foo')).toEqual(dependenciesByProjectId.project2.get('foo'))
  expect(dependenciesByProjectId.project1.get('foo')).not.toEqual(dependenciesByProjectId.project3.get('foo'))
  expect(dependenciesByProjectId.project3.get('foo')).toEqual(dependenciesByProjectId.project4.get('foo'))
})
