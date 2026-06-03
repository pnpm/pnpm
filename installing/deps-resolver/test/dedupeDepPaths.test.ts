import { expect, test } from '@jest/globals'
import type { PkgIdWithPatchHash, PkgResolutionId, ProjectRootDir } from '@pnpm/types'

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
