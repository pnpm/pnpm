/// <reference path="../../../__typings__/index.d.ts" />
import { beforeAll, describe, expect, it, test } from '@jest/globals'
import type {
  DepPath,
  PeerDependencyIssuesByProjects,
  PkgIdWithPatchHash,
  PkgResolutionId,
  ProjectRootDir,
} from '@pnpm/types'

import type { NodeId } from '../lib/nextNodeId.js'
import type { ChildrenMap, DependenciesTreeNode, PeerDependencies } from '../lib/resolveDependencies.js'
import { type PartialResolvedPackage, resolvePeers } from '../lib/resolvePeers.js'

test('resolve peer dependencies of cyclic dependencies', async () => {
  const fooPkg = {
    name: 'foo',
    pkgIdWithPatchHash: 'foo/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      qar: { version: '1.0.0' },
      zoo: { version: '1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const barPkg = {
    name: 'bar',
    pkgIdWithPatchHash: 'bar/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      foo: { version: '1.0.0' },
      zoo: { version: '1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const { dependenciesGraph } = await resolvePeers({
    allPeerDepNames: new Set(['foo', 'bar', 'qar', 'zoo']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['foo', '>foo/1.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: '',
      },
    ],
    resolvedImporters: {},
    dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      ['>foo/1.0.0>' as NodeId, {
        children: {
          bar: '>foo/1.0.0>bar/1.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: fooPkg,
        depth: 0,
      }],
      ['>foo/1.0.0>bar/1.0.0>' as NodeId, {
        children: {
          qar: '>foo/1.0.0>bar/1.0.0>qar/1.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: barPkg,
        depth: 1,
      }],
      ['>foo/1.0.0>bar/1.0.0>qar/1.0.0>' as NodeId, {
        children: {
          zoo: '>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: {
          name: 'qar',
          pkgIdWithPatchHash: 'qar/1.0.0' as PkgIdWithPatchHash,
          version: '1.0.0',
          peerDependencies: {
            foo: { version: '1.0.0' },
            bar: { version: '1.0.0' },
          },
          id: '' as PkgResolutionId,
        },
        depth: 2,
      }],
      ['>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>' as NodeId, {
        children: {
          foo: '>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>foo/1.0.0>' as NodeId,
          bar: '>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>bar/1.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: {
          name: 'zoo',
          pkgIdWithPatchHash: 'zoo/1.0.0' as PkgIdWithPatchHash,
          version: '1.0.0',
          peerDependencies: {
            qar: { version: '1.0.0' },
          },
          id: '' as PkgResolutionId,
        },
        depth: 3,
      }],
      ['>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>foo/1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 4,
      }],
      ['>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>bar/1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: barPkg,
        depth: 4,
      }],
    ]),
    virtualStoreDir: '',
    lockfileDir: '',
    virtualStoreDirMaxLength: 120,
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })
  expect(Object.keys(dependenciesGraph)).toStrictEqual([
    'foo/1.0.0',
    'bar/1.0.0(foo/1.0.0)',
    'qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0)',
    'zoo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0))',
    'foo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0))(zoo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0)))',
    'bar/1.0.0(foo/1.0.0)(zoo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0)))',
  ])
})

test('transitive pending peer uses provider final suffix', async () => {
  const aPkg = {
    name: 'a',
    pkgIdWithPatchHash: 'a@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      c: { version: '1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const bPkg = {
    name: 'b',
    pkgIdWithPatchHash: 'b@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      a: { version: '1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const cPkg = {
    name: 'c',
    pkgIdWithPatchHash: 'c@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {} as PeerDependencies,
    id: '' as PkgResolutionId,
  }
  const xPkg = {
    name: 'x',
    pkgIdWithPatchHash: 'x@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      b: { version: '1.0.0' },
    },
    id: '' as PkgResolutionId,
  }

  const { dependenciesGraph } = await resolvePeers({
    allPeerDepNames: new Set(['a', 'b', 'c']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['a', '>a@1.0.0>' as NodeId],
          ['c', '>c@1.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: '',
      },
    ],
    resolvedImporters: {},
    dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      ['>a@1.0.0>' as NodeId, {
        children: {
          b: '>a@1.0.0>b@1.0.0>' as NodeId,
          x: '>a@1.0.0>x@1.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: aPkg,
        depth: 0,
      }],
      ['>a@1.0.0>b@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: bPkg,
        depth: 1,
      }],
      ['>a@1.0.0>x@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: xPkg,
        depth: 1,
      }],
      ['>c@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: cPkg,
        depth: 0,
      }],
    ]),
    virtualStoreDir: '',
    lockfileDir: '',
    virtualStoreDirMaxLength: 120,
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })

  expect(Object.keys(dependenciesGraph)).toContain('x@1.0.0(b@1.0.0(a@1.0.0(c@1.0.0)))')
  expect(Object.keys(dependenciesGraph)).not.toContain('x@1.0.0(b@1.0.0(a@1.0.0))')
})

test('when a package is referenced twice in the dependencies graph and one of the times it cannot resolve its peers, still try to resolve it in the other occurrence', async () => {
  const fooPkg = {
    name: 'foo',
    pkgIdWithPatchHash: 'foo/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      qar: { version: '1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const barPkg = {
    name: 'bar',
    pkgIdWithPatchHash: 'bar/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {} as PeerDependencies,
    id: '' as PkgResolutionId,
  }
  const zooPkg = {
    name: 'zoo',
    pkgIdWithPatchHash: 'zoo/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {} as PeerDependencies,
    id: '' as PkgResolutionId,
  }
  const { dependenciesGraph } = await resolvePeers({
    allPeerDepNames: new Set(['foo', 'bar', 'qar', 'zoo']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['zoo', '>zoo/1.0.0>' as NodeId],
          ['bar', '>bar/1.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: '',
      },
    ],
    resolvedImporters: {},
    dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      ['>zoo/1.0.0>' as NodeId, {
        children: {
          foo: '>zoo/1.0.0>foo/1.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: zooPkg,
        depth: 0,
      }],
      ['>zoo/1.0.0>foo/1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 1,
      }],
      ['>bar/1.0.0>' as NodeId, {
        children: {
          zoo: '>bar/1.0.0>zoo/1.0.0>' as NodeId,
          qar: '>bar/1.0.0>qar/1.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: barPkg,
        depth: 0,
      }],
      ['>bar/1.0.0>zoo/1.0.0>' as NodeId, {
        children: {
          foo: '>bar/1.0.0>zoo/1.0.0>foo/1.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: zooPkg,
        depth: 1,
      }],
      ['>bar/1.0.0>zoo/1.0.0>foo/1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 2,
      }],
      ['>bar/1.0.0>qar/1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: {
          name: 'qar',
          pkgIdWithPatchHash: 'qar/1.0.0' as PkgIdWithPatchHash,
          version: '1.0.0',
          peerDependencies: {},
          id: '' as PkgResolutionId,
        },
        depth: 1,
      }],
    ]),
    virtualStoreDir: '',
    virtualStoreDirMaxLength: 120,
    lockfileDir: '',
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })
  expect(Object.keys(dependenciesGraph).sort()).toStrictEqual([
    'bar/1.0.0',
    'foo/1.0.0',
    'foo/1.0.0(qar/1.0.0)',
    'qar/1.0.0',
    'zoo/1.0.0',
    'zoo/1.0.0(qar/1.0.0)',
  ])
})

describe('peer dependency issues', () => {
  let peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
  beforeAll(async () => {
    const fooPkg = {
      name: 'foo',
      pkgIdWithPatchHash: 'foo/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {
        peer: { version: '1' },
      },
      id: '' as PkgResolutionId,
    }
    const fooWithOptionalPeer = {
      name: 'foo',
      pkgIdWithPatchHash: 'foo/2.0.0' as PkgIdWithPatchHash,
      version: '2.0.0',
      peerDependencies: {
        peer: { version: '1', optional: true },
      },
      id: '' as PkgResolutionId,
    }
    const barPkg = {
      name: 'bar',
      pkgIdWithPatchHash: 'bar/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {
        peer: { version: '2' },
      },
      id: '' as PkgResolutionId,
    }
    const barWithOptionalPeer = {
      name: 'bar',
      pkgIdWithPatchHash: 'bar/2.0.0' as PkgIdWithPatchHash,
      version: '2.0.0',
      peerDependencies: {
        peer: { version: '2', optional: true },
      },
      id: '' as PkgResolutionId,
    }
    const qarPkg = {
      name: 'qar',
      pkgIdWithPatchHash: 'qar/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {
        peer: { version: '^2.2.0' },
      },
      id: '' as PkgResolutionId,
    }
    peerDependencyIssuesByProjects = (await resolvePeers({
      allPeerDepNames: new Set(),
      projects: [
        {
          directNodeIdsByAlias: new Map([
            ['foo', '>project1>foo/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project1' as PkgResolutionId,
        },
        {
          directNodeIdsByAlias: new Map([
            ['bar', '>project2>bar/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project2' as PkgResolutionId,
        },
        {
          directNodeIdsByAlias: new Map([
            ['foo', '>project3>foo/1.0.0>' as NodeId],
            ['bar', '>project3>bar/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project3' as PkgResolutionId,
        },
        {
          directNodeIdsByAlias: new Map([
            ['bar', '>project4>bar/1.0.0>' as NodeId],
            ['qar', '>project4>qar/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project4' as PkgResolutionId,
        },
        {
          directNodeIdsByAlias: new Map([
            ['foo', '>project5>foo/1.0.0>' as NodeId],
            ['bar', '>project5>bar/2.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project5' as PkgResolutionId,
        },
        {
          directNodeIdsByAlias: new Map([
            ['foo', '>project6>foo/2.0.0>' as NodeId],
            ['bar', '>project6>bar/2.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project6' as PkgResolutionId,
        },
      ],
      resolvedImporters: {},
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ['>project1>foo/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: fooPkg,
          depth: 0,
        }],
        ['>project2>bar/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: barPkg,
          depth: 0,
        }],
        ['>project3>foo/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: fooPkg,
          depth: 0,
        }],
        ['>project3>bar/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: barPkg,
          depth: 0,
        }],
        ['>project4>bar/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: barPkg,
          depth: 0,
        }],
        ['>project4>qar/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: qarPkg,
          depth: 0,
        }],
        ['>project5>foo/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: fooPkg,
          depth: 0,
        }],
        ['>project5>bar/2.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: barWithOptionalPeer,
          depth: 0,
        }],
        ['>project6>foo/2.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: fooWithOptionalPeer,
          depth: 0,
        }],
        ['>project6>bar/2.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: barWithOptionalPeer,
          depth: 0,
        }],
      ]),
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })).peerDependencyIssuesByProjects
  })
  it('should find peer dependency conflicts', () => {
    expect(peerDependencyIssuesByProjects['project3'].conflicts).toStrictEqual(['peer'])
  })
  it('should find peer dependency conflicts when the peer is an optional peer of one of the dependencies', () => {
    expect(peerDependencyIssuesByProjects['project5'].conflicts).toStrictEqual(['peer'])
  })
  it('should ignore conflicts between missing optional peer dependencies', () => {
    expect(peerDependencyIssuesByProjects['project6'].conflicts).toStrictEqual([])
  })
  it('should pick the single wanted peer dependency range', () => {
    expect(peerDependencyIssuesByProjects['project1'].intersections)
      .toStrictEqual({ peer: '1' })
    expect(peerDependencyIssuesByProjects['project2'].intersections)
      .toStrictEqual({ peer: '2' })
  })
  it('should return the intersection of two compatible ranges', () => {
    expect(peerDependencyIssuesByProjects['project4'].intersections)
      .toStrictEqual({ peer: '>=2.2.0 <3.0.0-0' })
  })
})

describe('unmet peer dependency issues', () => {
  let peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
  beforeAll(async () => {
    peerDependencyIssuesByProjects = (await resolvePeers({
      allPeerDepNames: new Set(),
      projects: [
        {
          directNodeIdsByAlias: new Map([
            ['foo', '>project1>foo/1.0.0>' as NodeId],
            ['peer1', '>project1>peer1/1.0.0-rc.0>' as NodeId],
            ['peer2', '>project1>peer2/1.1.0-rc.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project1' as PkgResolutionId,
        },
      ],
      resolvedImporters: {},
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ['>project1>foo/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: {
            name: 'foo',
            version: '1.0.0',
            pkgIdWithPatchHash: 'foo/1.0.0' as PkgIdWithPatchHash,
            peerDependencies: {
              peer1: { version: '*' },
              peer2: { version: '>=1' },
            },
            id: '' as PkgResolutionId,
          },
          depth: 0,
        }],
        ['>project1>peer1/1.0.0-rc.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: {
            name: 'peer1',
            version: '1.0.0-rc.0',
            pkgIdWithPatchHash: 'peer/1.0.0-rc.0' as PkgIdWithPatchHash,
            peerDependencies: {},
            id: '' as PkgResolutionId,
          },
          depth: 0,
        }],
        ['>project1>peer2/1.1.0-rc.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: {
            name: 'peer2',
            version: '1.1.0-rc.0',
            pkgIdWithPatchHash: 'peer/1.1.0-rc.0' as PkgIdWithPatchHash,
            peerDependencies: {},
            id: '' as PkgResolutionId,
          },
          depth: 0,
        }],
      ]),
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })).peerDependencyIssuesByProjects
  })
  it('should not warn when the found package has prerelease version and the wanted range is *', () => {
    expect(peerDependencyIssuesByProjects).not.toHaveProperty(['project1', 'bad', 'peer1'])
  })
  it('should not warn when the found package is a prerelease version but satisfies the range', () => {
    expect(peerDependencyIssuesByProjects).not.toHaveProperty(['project1', 'bad', 'peer2'])
  })
})

describe('unmet peer dependency issue resolved from subdependency', () => {
  let peerDependencyIssuesByProjects: PeerDependencyIssuesByProjects
  beforeAll(async () => {
    peerDependencyIssuesByProjects = (await resolvePeers({
      allPeerDepNames: new Set(['dep']),
      projects: [
        {
          directNodeIdsByAlias: new Map([
            ['foo', '>project>foo/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project' as PkgResolutionId,
        },
      ],
      resolvedImporters: {},
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ['>project>foo/1.0.0>' as NodeId, {
          children: {
            dep: '>project>foo/1.0.0>dep/1.0.0>' as NodeId,
            bar: '>project>foo/1.0.0>bar/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: {
            name: 'foo',
            pkgIdWithPatchHash: 'foo/1.0.0' as PkgIdWithPatchHash,
            version: '1.0.0',
            peerDependencies: {},
            id: '' as PkgResolutionId,
          },
          depth: 0,
        }],
        ['>project>foo/1.0.0>dep/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: {
            name: 'dep',
            pkgIdWithPatchHash: 'dep/1.0.0' as PkgIdWithPatchHash,
            version: '1.0.0',
            peerDependencies: {},
            id: '' as PkgResolutionId,
          },
          depth: 1,
        }],
        ['>project>foo/1.0.0>bar/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: {
            name: 'bar',
            pkgIdWithPatchHash: 'bar/1.0.0' as PkgIdWithPatchHash,
            version: '1.0.0',
            peerDependencies: {
              dep: { version: '10' },
            },
            id: '' as PkgResolutionId,
          },
          depth: 1,
        }],
      ]),
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })).peerDependencyIssuesByProjects
  })
  it('should return from where the bad peer dependency is resolved', () => {
    expect(peerDependencyIssuesByProjects.project.bad.dep[0].resolvedFrom).toStrictEqual([{ name: 'foo', version: '1.0.0' }])
  })
})

test('resolve peer dependencies with npm aliases', async () => {
  const fooPkg = {
    name: 'foo',
    pkgIdWithPatchHash: 'foo/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      bar: { version: '1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const fooAliasPkg = {
    name: 'foo',
    pkgIdWithPatchHash: 'foo/2.0.0' as PkgIdWithPatchHash,
    version: '2.0.0',
    peerDependencies: {
      bar: { version: '2.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const barPkg = {
    name: 'bar',
    pkgIdWithPatchHash: 'bar/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {},
    id: '' as PkgResolutionId,
  }
  const barAliasPkg = {
    name: 'bar',
    pkgIdWithPatchHash: 'bar/2.0.0' as PkgIdWithPatchHash,
    version: '2.0.0',
    peerDependencies: {},
    id: '' as PkgResolutionId,
  }
  const { dependenciesGraph } = await resolvePeers({
    allPeerDepNames: new Set(['bar']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['foo', '>foo/1.0.0>' as NodeId],
          ['bar', '>bar/1.0.0>' as NodeId],
          ['foo-next', '>foo/2.0.0>' as NodeId],
          ['bar-next', '>bar/2.0.0>' as NodeId],
        ]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: '' as PkgResolutionId,
      },
    ],
    resolvedImporters: {},
    dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      ['>foo/1.0.0>' as NodeId, {
        children: {
          bar: '>foo/1.0.0>bar/1.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: fooPkg,
        depth: 0,
      }],
      ['>foo/1.0.0>bar/1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: barPkg,
        depth: 1,
      }],
      ['>foo/2.0.0>' as NodeId, {
        children: {
          bar: '>foo/2.0.0>bar/2.0.0>' as NodeId,
        },
        installable: true,
        resolvedPackage: fooAliasPkg,
        depth: 0,
      }],
      ['>foo/2.0.0>bar/2.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: barAliasPkg,
        depth: 1,
      }],
      ['>bar/1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: barPkg,
        depth: 0,
      }],
      ['>bar/2.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: barAliasPkg,
        depth: 0,
      }],
    ]),
    virtualStoreDir: '',
    virtualStoreDirMaxLength: 120,
    lockfileDir: '',
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })
  expect(Object.keys(dependenciesGraph).sort()).toStrictEqual([
    'bar/1.0.0',
    'bar/2.0.0',
    'foo/1.0.0(bar/1.0.0)',
    'foo/2.0.0(bar/2.0.0)',
  ])
})

describe('locked peer provider preferences', () => {
  const currentPeerNodeId = '>peer/1.0.0>' as NodeId
  const retainedPeerNodeId = '>retainer/1.0.0>peer/2.0.0>' as NodeId
  const retainerNodeId = '>retainer/1.0.0>' as NodeId
  const wrapperNodeId = '>wrapper/1.0.0>' as NodeId
  const consumerNodeId = '>wrapper/1.0.0>consumer/1.0.0>' as NodeId

  const peer1Pkg = {
    name: 'peer',
    pkgIdWithPatchHash: 'peer/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {} as PeerDependencies,
    id: '' as PkgResolutionId,
  }
  const peer2Pkg = {
    name: 'peer',
    pkgIdWithPatchHash: 'peer/2.0.0' as PkgIdWithPatchHash,
    version: '2.0.0',
    peerDependencies: {} as PeerDependencies,
    id: '' as PkgResolutionId,
  }
  const wrapperPkg = {
    name: 'wrapper',
    pkgIdWithPatchHash: 'wrapper/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {} as PeerDependencies,
    id: '' as PkgResolutionId,
  }
  const retainerPkg = {
    name: 'retainer',
    pkgIdWithPatchHash: 'retainer/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {} as PeerDependencies,
    id: '' as PkgResolutionId,
  }
  const consumerPkg = {
    name: 'consumer',
    pkgIdWithPatchHash: 'consumer/1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      peer: { version: '>=1' },
    },
    id: '' as PkgResolutionId,
  }

  function createTree (
    declaredPeerNodeId?: NodeId,
    currentPeerWasPreviouslyLocked = true,
    peerRange = '>=1'
  ): Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>> {
    const wrapperChildren: ChildrenMap = { consumer: consumerNodeId }
    if (declaredPeerNodeId != null) wrapperChildren.peer = declaredPeerNodeId
    return new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      [currentPeerNodeId, {
        children: {},
        installable: true,
        previousDepPath: currentPeerWasPreviouslyLocked ? 'peer/1.0.0' as DepPath : undefined,
        resolvedPackage: peer1Pkg,
        depth: 0,
      }],
      [retainerNodeId, {
        children: { peer: retainedPeerNodeId },
        installable: true,
        resolvedPackage: retainerPkg,
        depth: 0,
      }],
      [retainedPeerNodeId, {
        children: {},
        installable: true,
        previousDepPath: 'peer/2.0.0' as DepPath,
        resolvedPackage: peer2Pkg,
        depth: 1,
      }],
      [wrapperNodeId, {
        children: wrapperChildren,
        dependencyNamesWhoseCurrentProviderMustWin: declaredPeerNodeId == null ? undefined : new Set(['peer']),
        installable: true,
        resolvedPackage: wrapperPkg,
        depth: 0,
      }],
      [consumerNodeId, {
        children: {},
        installable: true,
        lockedPeerContext: { peer: 'peer/2.0.0' as DepPath },
        resolvedPackage: {
          ...consumerPkg,
          peerDependencies: {
            peer: { version: peerRange },
          },
        },
        depth: 1,
      }],
    ])
  }

  function options (
    dependenciesTree: Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>,
    directNodeIdsByAlias: Map<string, NodeId>,
    declaredDirectDependencies: Set<string> = new Set(),
    explicitlyRequestedDirectDependencies: Set<string> = new Set()
  ) {
    return {
      allPeerDepNames: new Set(['peer']),
      projects: [{
        directNodeIdsByAlias,
        declaredDirectDependencies,
        explicitlyRequestedDirectDependencies,
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: '',
      }],
      resolvedImporters: {},
      dependenciesTree,
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set<string>(),
    }
  }

  test('prefers a compatible locked provider that remains reachable in the current graph', async () => {
    const resolutionOpts = options(createTree(), new Map([
      ['peer', currentPeerNodeId],
      ['retainer', retainerNodeId],
      ['wrapper', wrapperNodeId],
    ]))
    const initial = await resolvePeers(resolutionOpts)
    const preferred = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })

    expect(initial.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeTruthy()
    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeTruthy()
  })

  test('does not replace a newly resolved nested peer provider', async () => {
    const resolutionOpts = options(createTree(currentPeerNodeId), new Map([
      ['retainer', retainerNodeId],
      ['wrapper', wrapperNodeId],
    ]))
    const initial = await resolvePeers(resolutionOpts)
    const preferred = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })

    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeTruthy()
    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
  })

  test('does not replace a newly declared importer peer provider', async () => {
    const resolutionOpts = options(createTree(undefined, false), new Map([
      ['consumer', consumerNodeId],
      ['peer', currentPeerNodeId],
      ['retainer', retainerNodeId],
    ]), new Set(['consumer', 'peer', 'retainer']))
    const initial = await resolvePeers(resolutionOpts)
    const preferred = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })

    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeTruthy()
    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
  })

  test('does not replace an explicitly updated importer peer provider that existed before', async () => {
    const resolutionOpts = options(createTree(), new Map([
      ['consumer', consumerNodeId],
      ['peer', currentPeerNodeId],
      ['retainer', retainerNodeId],
    ]), new Set(['consumer', 'peer', 'retainer']), new Set(['peer']))
    const initial = await resolvePeers(resolutionOpts)
    const preferred = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })

    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeTruthy()
    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
  })

  test('does not replace an explicitly requested importer provider for a nested consumer', async () => {
    const resolutionOpts = options(createTree(), new Map([
      ['peer', currentPeerNodeId],
      ['retainer', retainerNodeId],
      ['wrapper', wrapperNodeId],
    ]), new Set(['peer', 'retainer', 'wrapper']), new Set(['peer']))
    const initial = await resolvePeers(resolutionOpts)
    const preferred = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })

    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeTruthy()
    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
  })

  test('does not replace an explicitly requested workspace root provider', async () => {
    const resolutionOpts = options(createTree(), new Map([
      ['consumer', consumerNodeId],
      ['retainer', retainerNodeId],
    ]))
    resolutionOpts.projects.unshift({
      directNodeIdsByAlias: new Map([['peer', currentPeerNodeId]]),
      declaredDirectDependencies: new Set(['peer']),
      explicitlyRequestedDirectDependencies: new Set(['peer']),
      topParents: [],
      rootDir: '' as ProjectRootDir,
      id: '.',
    })
    const initial = await resolvePeers({
      ...resolutionOpts,
      resolvePeersFromWorkspaceRoot: true,
    })
    const preferred = await resolvePeers({
      ...resolutionOpts,
      resolvePeersFromWorkspaceRoot: true,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })

    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeTruthy()
    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
  })

  test('does not replace an explicitly requested importer peer provider installed by alias', async () => {
    const resolutionOpts = options(createTree(), new Map([
      ['peer-alias', currentPeerNodeId],
      ['retainer', retainerNodeId],
      ['wrapper', wrapperNodeId],
    ]), new Set(['peer-alias', 'retainer', 'wrapper']), new Set(['peer-alias']))
    const initial = await resolvePeers(resolutionOpts)
    const preferred = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })

    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeTruthy()
    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
  })

  test('does not replace an existing importer peer provider installed by alias', async () => {
    const resolutionOpts = options(createTree(), new Map([
      ['peer-alias', currentPeerNodeId],
      ['retainer', retainerNodeId],
      ['wrapper', wrapperNodeId],
    ]), new Set(['peer-alias', 'retainer', 'wrapper']))
    const initial = await resolvePeers(resolutionOpts)
    const preferred = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })

    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeTruthy()
    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
  })

  test('a provider pinned for a childless consumer does not leak to the consumer\'s siblings', async () => {
    const victimNodeId = '>wrapper/1.0.0>victim/1.0.0>' as NodeId
    const victimPkg = {
      name: 'victim',
      pkgIdWithPatchHash: 'victim/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {
        peer: { version: '>=1', optional: true },
      },
      id: '' as PkgResolutionId,
    }
    // The two orders simulate the dependency tree arriving in different
    // resolution orders (network timing). The victim's resolution must not
    // depend on whether it is processed before or after the consumer whose
    // locked context pins the provider.
    const wrapperChildrenVariants: ChildrenMap[] = [
      { consumer: consumerNodeId, victim: victimNodeId },
      { victim: victimNodeId, consumer: consumerNodeId },
    ]
    for (const wrapperChildren of wrapperChildrenVariants) {
      const dependenciesTree = new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [retainerNodeId, {
          children: { peer: retainedPeerNodeId },
          installable: true,
          resolvedPackage: retainerPkg,
          depth: 0,
        }],
        [retainedPeerNodeId, {
          children: {},
          installable: true,
          previousDepPath: 'peer/2.0.0' as DepPath,
          resolvedPackage: peer2Pkg,
          depth: 1,
        }],
        [wrapperNodeId, {
          children: wrapperChildren,
          installable: true,
          resolvedPackage: wrapperPkg,
          depth: 0,
        }],
        [consumerNodeId, {
          children: {},
          installable: true,
          lockedPeerContext: { peer: 'peer/2.0.0' as DepPath },
          resolvedPackage: consumerPkg,
          depth: 1,
        }],
        [victimNodeId, {
          children: {},
          installable: true,
          resolvedPackage: victimPkg,
          depth: 1,
        }],
      ])
      const resolutionOpts = options(dependenciesTree, new Map([
        ['retainer', retainerNodeId],
        ['wrapper', wrapperNodeId],
      ]))
      // eslint-disable-next-line no-await-in-loop
      const initial = await resolvePeers(resolutionOpts)
      // eslint-disable-next-line no-await-in-loop
      const preferred = await resolvePeers({
        ...resolutionOpts,
        resolvedPeerProviderPaths: initial.pathsByNodeId,
      })

      expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeTruthy()
      expect(preferred.dependenciesGraph['victim/1.0.0' as DepPath]).toBeTruthy()
      expect(preferred.dependenciesGraph['victim/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
    }
  })

  test('does not reuse a locked provider outside the current peer range', async () => {
    const resolutionOpts = options(createTree(undefined, true, '^1.0.0'), new Map([
      ['peer', currentPeerNodeId],
      ['retainer', retainerNodeId],
      ['wrapper', wrapperNodeId],
    ]))
    const initial = await resolvePeers(resolutionOpts)
    const preferred = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })

    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeTruthy()
    expect(preferred.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
  })
})

describe('dedupePeers', () => {
  test('uses version-only peer suffixes without nested dep paths', async () => {
    // Simulates: react@18, @emotion/react@11(peer: react), @emotion/styled@11(peer: react, @emotion/react)
    // Without dedupePeers: @emotion/styled gets suffix (@emotion/react@11(react@18))(react@18) — nested dep paths
    // With dedupePeers: @emotion/styled gets suffix (@emotion/react@11.0.0)(react@18.0.0) — version-only, no nesting
    const reactPkg = {
      name: 'react',
      pkgIdWithPatchHash: 'react/18.0.0' as PkgIdWithPatchHash,
      version: '18.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const emotionReactPkg = {
      name: '@emotion/react',
      pkgIdWithPatchHash: '@emotion/react/11.0.0' as PkgIdWithPatchHash,
      version: '11.0.0',
      peerDependencies: {
        react: { version: '>=16' },
      },
      id: '' as PkgResolutionId,
    }
    const emotionStyledPkg = {
      name: '@emotion/styled',
      pkgIdWithPatchHash: '@emotion/styled/11.0.0' as PkgIdWithPatchHash,
      version: '11.0.0',
      peerDependencies: {
        react: { version: '>=16' },
        '@emotion/react': { version: '>=11' },
      },
      id: '' as PkgResolutionId,
    }
    const { dependenciesGraph } = await resolvePeers({
      allPeerDepNames: new Set(['react', '@emotion/react', '@emotion/styled']),
      dedupePeers: true,
      projects: [
        {
          directNodeIdsByAlias: new Map([
            ['react', '>react/18.0.0>' as NodeId],
            ['@emotion/react', '>@emotion/react/11.0.0>' as NodeId],
            ['@emotion/styled', '>@emotion/styled/11.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: '',
        },
      ],
      resolvedImporters: {},
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ['>react/18.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: reactPkg,
          depth: 0,
        }],
        ['>@emotion/react/11.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: emotionReactPkg,
          depth: 0,
        }],
        ['>@emotion/styled/11.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: emotionStyledPkg,
          depth: 0,
        }],
      ]),
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })
    expect(Object.keys(dependenciesGraph).sort()).toStrictEqual([
      '@emotion/react/11.0.0(react@18.0.0)',
      '@emotion/styled/11.0.0(@emotion/react@11.0.0)(react@18.0.0)',
      'react/18.0.0',
    ])
  })

  test('transitive peers use version-only suffixes', async () => {
    // A depends on B (peer: C). A has no peers itself.
    // Without dedupePeers: A gets suffix (c/1.0.0) — full dep path
    // With dedupePeers: A gets suffix (c@1.0.0) — version-only
    const aPkg = {
      name: 'a',
      pkgIdWithPatchHash: 'a/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const bPkg = {
      name: 'b',
      pkgIdWithPatchHash: 'b/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {
        c: { version: '1.0.0' },
      },
      id: '' as PkgResolutionId,
    }
    const cPkg = {
      name: 'c',
      pkgIdWithPatchHash: 'c/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const { dependenciesGraph } = await resolvePeers({
      allPeerDepNames: new Set(['c']),
      dedupePeers: true,
      projects: [
        {
          directNodeIdsByAlias: new Map([
            ['a', '>a/1.0.0>' as NodeId],
            ['c', '>c/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: '',
        },
      ],
      resolvedImporters: {},
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ['>a/1.0.0>' as NodeId, {
          children: {
            b: '>a/1.0.0>b/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: aPkg,
          depth: 0,
        }],
        ['>a/1.0.0>b/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: bPkg,
          depth: 1,
        }],
        ['>c/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: cPkg,
          depth: 0,
        }],
      ]),
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })
    expect(Object.keys(dependenciesGraph).sort()).toStrictEqual([
      'a/1.0.0(c@1.0.0)',
      'b/1.0.0(c@1.0.0)',
      'c/1.0.0',
    ])
  })

  test('multi-project: different peer versions produce different instances', async () => {
    // project-a has react@17, project-b has react@18
    // Both have plugin@1 (peer: react)
    const react17Pkg = {
      name: 'react',
      pkgIdWithPatchHash: 'react/17.0.0' as PkgIdWithPatchHash,
      version: '17.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const react18Pkg = {
      name: 'react',
      pkgIdWithPatchHash: 'react/18.0.0' as PkgIdWithPatchHash,
      version: '18.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const pluginPkg = {
      name: 'plugin',
      pkgIdWithPatchHash: 'plugin/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {
        react: { version: '>=16' },
      },
      id: '' as PkgResolutionId,
    }
    const { dependenciesGraph, dependenciesByProjectId } = await resolvePeers({
      allPeerDepNames: new Set(['react']),
      dedupePeers: true,
      projects: [
        {
          directNodeIdsByAlias: new Map([
            ['react', '>project-a>react/17.0.0>' as NodeId],
            ['plugin', '>project-a>plugin/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project-a',
        },
        {
          directNodeIdsByAlias: new Map([
            ['react', '>project-b>react/18.0.0>' as NodeId],
            ['plugin', '>project-b>plugin/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: 'project-b',
        },
      ],
      resolvedImporters: {},
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ['>project-a>react/17.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: react17Pkg,
          depth: 0,
        }],
        ['>project-a>plugin/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: pluginPkg,
          depth: 0,
        }],
        ['>project-b>react/18.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: react18Pkg,
          depth: 0,
        }],
        ['>project-b>plugin/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: pluginPkg,
          depth: 0,
        }],
      ]),
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })
    // Plugin has two instances — one per react version
    expect(Object.keys(dependenciesGraph).sort()).toStrictEqual([
      'plugin/1.0.0(react@17.0.0)',
      'plugin/1.0.0(react@18.0.0)',
      'react/17.0.0',
      'react/18.0.0',
    ])
    // Each project gets the correct instance
    expect(dependenciesByProjectId['project-a'].get('plugin')).toBe('plugin/1.0.0(react@17.0.0)')
    expect(dependenciesByProjectId['project-b'].get('plugin')).toBe('plugin/1.0.0(react@18.0.0)')
  })

  // https://github.com/pnpm/pnpm/issues/12079
  test("a peer's own peer is shared with a sibling that peer-depends both", async () => {
    // plugin peer-depends both parser and typescript; parser peer-depends typescript.
    // So plugin's parser and plugin's typescript must agree. A top-level parser that
    // resolved typescript@2.0.0 must not shadow umbrella's own parser, which resolves
    // typescript@1.0.0 — the version that plugin itself uses.
    const ts1Pkg = {
      name: 'typescript',
      pkgIdWithPatchHash: 'typescript/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const ts2Pkg = {
      name: 'typescript',
      pkgIdWithPatchHash: 'typescript/2.0.0' as PkgIdWithPatchHash,
      version: '2.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const parserPkg = {
      name: 'parser',
      pkgIdWithPatchHash: 'parser/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: { typescript: { version: '*' } },
      id: '' as PkgResolutionId,
    }
    const pluginPkg = {
      name: 'plugin',
      pkgIdWithPatchHash: 'plugin/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: { parser: { version: '*' }, typescript: { version: '*' } },
      id: '' as PkgResolutionId,
    }
    const umbrellaPkg = {
      name: 'umbrella',
      pkgIdWithPatchHash: 'umbrella/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: { typescript: { version: '*' } },
      id: '' as PkgResolutionId,
    }
    const appPkg = {
      name: 'app',
      pkgIdWithPatchHash: 'app/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const { dependenciesGraph } = await resolvePeers({
      allPeerDepNames: new Set(['typescript', 'parser']),
      projects: [
        {
          directNodeIdsByAlias: new Map([
            ['typescript', '>typescript/2.0.0>' as NodeId],
            ['parser', '>parser/1.0.0>' as NodeId],
            ['app', '>app/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: '.',
        },
      ],
      resolvedImporters: {},
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ['>typescript/2.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: ts2Pkg,
          depth: 0,
        }],
        ['>parser/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: parserPkg,
          depth: 0,
        }],
        ['>app/1.0.0>' as NodeId, {
          children: {
            typescript: '>app/1.0.0>typescript/1.0.0>' as NodeId,
            umbrella: '>app/1.0.0>umbrella/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: appPkg,
          depth: 0,
        }],
        ['>app/1.0.0>typescript/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: ts1Pkg,
          depth: 1,
        }],
        ['>app/1.0.0>umbrella/1.0.0>' as NodeId, {
          children: {
            plugin: '>app/1.0.0>umbrella/1.0.0>plugin/1.0.0>' as NodeId,
            parser: '>app/1.0.0>umbrella/1.0.0>parser/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: umbrellaPkg,
          depth: 1,
        }],
        ['>app/1.0.0>umbrella/1.0.0>plugin/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: pluginPkg,
          depth: 2,
        }],
        ['>app/1.0.0>umbrella/1.0.0>parser/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: parserPkg,
          depth: 2,
        }],
      ]),
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })
    // plugin and its parser both use typescript@1.0.0; the typescript@2.0.0 parser
    // from the top level is not pulled into plugin.
    const depPaths = Object.keys(dependenciesGraph)
    expect(depPaths).toContain('plugin/1.0.0(parser/1.0.0(typescript/1.0.0))(typescript/1.0.0)')
    expect(depPaths).not.toContain('plugin/1.0.0(parser/1.0.0(typescript/2.0.0))(typescript/1.0.0)')
  })

  // A cycle re-entry of `a` (a→b→a) resolves against truncated children and must
  // not poison the purePkgs/peersCache verdict for `a`. If it did, the sibling
  // occurrence under `h` would short-circuit to empty and lose the transitive
  // peer `e` that `a` reaches through c→d, churning the lockfile by traversal
  // order. https://github.com/pnpm/pnpm/issues/5108
  test('cycle re-entry does not drop a sibling occurrence transitive peers', async () => {
    const aPkg = {
      name: 'a',
      pkgIdWithPatchHash: 'a/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const bPkg = {
      name: 'b',
      pkgIdWithPatchHash: 'b/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const cPkg = {
      name: 'c',
      pkgIdWithPatchHash: 'c/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const dPkg = {
      name: 'd',
      pkgIdWithPatchHash: 'd/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {
        e: { version: '1.0.0', optional: true },
      },
      id: '' as PkgResolutionId,
    }
    const gPkg = {
      name: 'g',
      pkgIdWithPatchHash: 'g/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const hPkg = {
      name: 'h',
      pkgIdWithPatchHash: 'h/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const { dependenciesGraph } = await resolvePeers({
      allPeerDepNames: new Set(['e']),
      projects: [
        {
          directNodeIdsByAlias: new Map([
            ['g', '>g/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: '',
        },
      ],
      resolvedImporters: {},
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ['>g/1.0.0>' as NodeId, {
          children: {
            a: '>g/1.0.0>a/1.0.0>' as NodeId,
            h: '>g/1.0.0>h/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: gPkg,
          depth: 0,
        }],
        ['>g/1.0.0>a/1.0.0>' as NodeId, {
          children: {
            b: '>g/1.0.0>a/1.0.0>b/1.0.0>' as NodeId,
            c: '>g/1.0.0>a/1.0.0>c/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: aPkg,
          depth: 1,
        }],
        ['>g/1.0.0>a/1.0.0>b/1.0.0>' as NodeId, {
          children: {
            a: '>g/1.0.0>a/1.0.0>b/1.0.0>a/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: bPkg,
          depth: 2,
        }],
        ['>g/1.0.0>a/1.0.0>b/1.0.0>a/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: aPkg,
          depth: 3,
        }],
        ['>g/1.0.0>a/1.0.0>c/1.0.0>' as NodeId, {
          children: {
            d: '>g/1.0.0>a/1.0.0>c/1.0.0>d/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: cPkg,
          depth: 2,
        }],
        ['>g/1.0.0>a/1.0.0>c/1.0.0>d/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: dPkg,
          depth: 3,
        }],
        ['>g/1.0.0>h/1.0.0>' as NodeId, {
          children: {
            a: '>g/1.0.0>h/1.0.0>a/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: hPkg,
          depth: 1,
        }],
        ['>g/1.0.0>h/1.0.0>a/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: aPkg,
          depth: 2,
        }],
      ]),
      virtualStoreDir: '',
      lockfileDir: '',
      virtualStoreDirMaxLength: 120,
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })

    const hEntry = dependenciesGraph['h/1.0.0' as DepPath]
    expect(hEntry).toBeTruthy()
    expect(Array.from(hEntry.transitivePeerDependencies)).toContain('e')

    const gEntry = dependenciesGraph['g/1.0.0' as DepPath]
    expect(gEntry).toBeTruthy()
    expect(Array.from(gEntry.transitivePeerDependencies)).toContain('e')
  })

  test('aliased dependency provides peer under real package name', async () => {
    const rootPkg = {
      name: 'root',
      pkgIdWithPatchHash: 'root/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const realPkg = {
      name: 'real-pkg',
      pkgIdWithPatchHash: 'real-pkg/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const childPkg = {
      name: 'child',
      pkgIdWithPatchHash: 'child/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {} as PeerDependencies,
      id: '' as PkgResolutionId,
    }
    const leafPkg = {
      name: 'leaf',
      pkgIdWithPatchHash: 'leaf/1.0.0' as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies: {
        'real-pkg': { version: '1.0.0' },
      },
      id: '' as PkgResolutionId,
    }
    const { peerDependencyIssuesByProjects } = await resolvePeers({
      allPeerDepNames: new Set(['real-pkg']),
      projects: [
        {
          directNodeIdsByAlias: new Map([
            ['root', '>root/1.0.0>' as NodeId],
          ]),
          topParents: [],
          rootDir: '' as ProjectRootDir,
          id: '',
        },
      ],
      resolvedImporters: {},
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ['>root/1.0.0>' as NodeId, {
          children: {
            'my-alias': '>root/1.0.0>real-pkg/1.0.0>' as NodeId,
            child: '>root/1.0.0>child/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: rootPkg,
          depth: 0,
        }],
        ['>root/1.0.0>real-pkg/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: realPkg,
          depth: 1,
        }],
        ['>root/1.0.0>child/1.0.0>' as NodeId, {
          children: {
            leaf: '>root/1.0.0>child/1.0.0>leaf/1.0.0>' as NodeId,
          },
          installable: true,
          resolvedPackage: childPkg,
          depth: 1,
        }],
        ['>root/1.0.0>child/1.0.0>leaf/1.0.0>' as NodeId, {
          children: {},
          installable: true,
          resolvedPackage: leafPkg,
          depth: 2,
        }],
      ]),
      virtualStoreDir: '',
      lockfileDir: '',
      virtualStoreDirMaxLength: 120,
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set(),
    })

    expect(peerDependencyIssuesByProjects['']?.missing?.['real-pkg']).toBeUndefined()
  })
})
