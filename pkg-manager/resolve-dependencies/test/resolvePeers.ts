/// <reference path="../../../__typings__/index.d.ts" />
import {
  type PkgResolutionId,
  type PeerDependencyIssuesByProjects,
  type PkgIdWithPatchHash,
  type ProjectRootDir,
} from '@pnpm/types'
import { type PartialResolvedPackage, resolvePeers } from '../lib/resolvePeers'
import { type DependenciesTreeNode, type PeerDependencies } from '../lib/resolveDependencies'
import { type NodeId } from '../lib/nextNodeId'

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
  })
  expect(Object.keys(dependenciesGraph)).toStrictEqual([
    'foo/1.0.0',
    'bar/1.0.0(foo/1.0.0)',
    'qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0)',
    'zoo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0))',
    'foo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0))(zoo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0)))',
    'bar/1.0.0(foo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0))(zoo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0))))(zoo/1.0.0(qar/1.0.0(bar/1.0.0(foo/1.0.0))(foo/1.0.0)))',
  ])
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
  })
  expect(Object.keys(dependenciesGraph).sort()).toStrictEqual([
    'bar/1.0.0',
    'bar/2.0.0',
    'foo/1.0.0(bar/1.0.0)',
    'foo/2.0.0(bar/2.0.0)',
  ])
})
