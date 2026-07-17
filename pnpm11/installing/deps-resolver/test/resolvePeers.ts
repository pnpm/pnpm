/// <reference path="../../../__typings__/index.d.ts" />
import path from 'node:path'

import { beforeAll, describe, expect, it, jest, test } from '@jest/globals'
import { createPeerDepGraphHash } from '@pnpm/deps.path'
import type {
  DepPath,
  PeerDependencyIssuesByProjects,
  PkgIdWithPatchHash,
  PkgResolutionId,
  ProjectRootDir,
} from '@pnpm/types'

import type { NodeId } from '../lib/nextNodeId.js'
import type { ChildrenMap, DependenciesTreeNode, LockedPeerContext, PeerDependencies } from '../lib/resolveDependencies.js'
import { type PartialResolvedPackage, pickPeerCleanupWinner, type ProjectToResolve, resolvePeers } from '../lib/resolvePeers.js'

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

test('peer cleanup selects a compatible child superset', () => {
  const subset = {
    children: { foo: 'foo/1.0.0' as DepPath },
    childKeys: new Set(['foo\0foo/1.0.0']),
    depPath: 'consumer/1.0.0(peer-a/1.0.0)' as DepPath,
  }
  const superset = {
    children: {
      bar: 'bar/1.0.0' as DepPath,
      foo: 'foo/1.0.0' as DepPath,
    },
    childKeys: new Set(['foo\0foo/1.0.0', 'bar\0bar/1.0.0']),
    depPath: 'consumer/1.0.0(peer-b/1.0.0)' as DepPath,
  }
  expect(pickPeerCleanupWinner([subset, superset])).toBe(superset)
})

test('peer cleanup rejects incompatible children', () => {
  const foo = {
    children: { foo: 'foo/1.0.0' as DepPath },
    childKeys: new Set(['foo\0foo/1.0.0']),
    depPath: 'consumer/1.0.0(peer-a/1.0.0)' as DepPath,
  }
  const bar = {
    children: { bar: 'bar/1.0.0' as DepPath },
    childKeys: new Set(['bar\0bar/1.0.0']),
    depPath: 'consumer/1.0.0(peer-b/1.0.0)' as DepPath,
  }
  expect(pickPeerCleanupWinner([foo, bar])).toBeUndefined()
})

test('peer cleanup rejects children assigned to different aliases', () => {
  const first = {
    children: { a: 'foo/1.0.0' as DepPath, b: 'bar/1.0.0' as DepPath },
    childKeys: new Set(['a\0foo/1.0.0', 'b\0bar/1.0.0']),
    depPath: 'consumer/1.0.0(peer-a/1.0.0)' as DepPath,
  }
  const second = {
    children: { a: 'bar/1.0.0' as DepPath, b: 'foo/1.0.0' as DepPath },
    childKeys: new Set(['a\0bar/1.0.0', 'b\0foo/1.0.0']),
    depPath: 'consumer/1.0.0(peer-b/1.0.0)' as DepPath,
  }
  expect(pickPeerCleanupWinner([first, second])).toBeUndefined()
})

test('peer cleanup breaks equal-child ties by depPath', () => {
  const later = {
    children: { foo: 'foo/1.0.0' as DepPath },
    childKeys: new Set(['foo\0foo/1.0.0']),
    depPath: 'consumer/1.0.0(peer-b/1.0.0)' as DepPath,
  }
  const earlier = {
    children: { foo: 'foo/1.0.0' as DepPath },
    childKeys: new Set(['foo\0foo/1.0.0']),
    depPath: 'consumer/1.0.0(peer-a/1.0.0)' as DepPath,
  }
  expect(pickPeerCleanupWinner([later, earlier])).toBe(earlier)
})

test('peer cleanup prefers an existing target depPath', () => {
  const remapped = {
    children: { foo: 'foo/1.0.0' as DepPath },
    childKeys: new Set(['foo\0foo/1.0.0']),
    depPath: 'consumer/1.0.0(peer-a/1.0.0)' as DepPath,
  }
  const existing = {
    children: { foo: 'foo/1.0.0' as DepPath },
    childKeys: new Set(['foo\0foo/1.0.0']),
    depPath: 'consumer/1.0.0' as DepPath,
  }
  expect(pickPeerCleanupWinner([remapped, existing], existing.depPath)).toBe(existing)
})

test('peer cleanup preserves the fresh child context when normalized children tie', () => {
  const normalizedStorybook = 'storybook/10.2.13' as DepPath
  const freshStorybook = 'storybook/10.2.13(react/18.3.1)' as DepPath
  const otherStorybook = 'storybook/10.2.13(react/19.2.7)' as DepPath
  const remapped = {
    children: { storybook: freshStorybook },
    childKeys: new Set([`storybook\0${normalizedStorybook}`]),
    depPath: 'consumer/1.0.0(supports-color/5.5.0)' as DepPath,
  }
  const existing = {
    children: { storybook: otherStorybook },
    childKeys: new Set([`storybook\0${normalizedStorybook}`]),
    depPath: 'consumer/1.0.0' as DepPath,
  }
  expect(pickPeerCleanupWinner(
    [existing, remapped],
    existing.depPath,
    { storybook: freshStorybook }
  )).toBe(remapped)
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

  function resolvedPeerNamesByNodeId (
    result: Awaited<ReturnType<typeof resolvePeers>>
  ): Map<NodeId, Set<string>> {
    const byNodeId = new Map<NodeId, Set<string>>()
    for (const [nodeId, depPath] of result.pathsByNodeId.entries()) {
      const node = result.dependenciesGraph[depPath]
      if (node != null && node.resolvedPeerNames.size > 0) {
        byNodeId.set(nodeId, node.resolvedPeerNames)
      }
    }
    return byNodeId
  }

  function fixturePkg (
    name: string,
    peerDependencies: PeerDependencies = {}
  ): PartialResolvedPackage {
    return {
      name,
      pkgIdWithPatchHash: `${name}/1.0.0` as PkgIdWithPatchHash,
      version: '1.0.0',
      peerDependencies,
      id: '' as PkgResolutionId,
    }
  }

  function fixtureNode (
    pkg: string | PartialResolvedPackage,
    overrides: {
      peerDependencies?: PeerDependencies
      children?: DependenciesTreeNode<PartialResolvedPackage>['children']
      depth?: number
      installable?: boolean
      previousDepPath?: DepPath
      lockedPeerContext?: LockedPeerContext
    } = {}
  ): DependenciesTreeNode<PartialResolvedPackage> {
    const resolvedPackage = typeof pkg === 'string' ? fixturePkg(pkg, overrides.peerDependencies) : pkg
    return {
      children: overrides.children ?? {},
      installable: overrides.installable ?? true,
      depth: overrides.depth ?? 0,
      resolvedPackage,
      ...(overrides.previousDepPath != null ? { previousDepPath: overrides.previousDepPath } : {}),
      ...(overrides.lockedPeerContext != null ? { lockedPeerContext: overrides.lockedPeerContext } : {}),
    }
  }

  function project (
    id: string,
    directNodeIdsByAlias: Map<string, NodeId>,
    overrides: Partial<ProjectToResolve> = {}
  ): ProjectToResolve {
    return {
      directNodeIdsByAlias,
      topParents: [],
      rootDir: '' as ProjectRootDir,
      id,
      ...overrides,
    }
  }

  function baseResolveOpts () {
    return {
      resolvedImporters: {},
      virtualStoreDir: '',
      virtualStoreDirMaxLength: 120,
      lockfileDir: '',
      peersSuffixMaxLength: 1000,
      workspaceProjectIds: new Set<string>(),
    }
  }

  async function resolveWithCleanup (
    resolutionOpts: Parameters<typeof resolvePeers>[0]
  ) {
    const initial = await resolvePeers({
      ...resolutionOpts,
      dedupePeerDependents: false,
    })
    const cleaned = await resolvePeers({
      ...resolutionOpts,
      dedupePeerDependents: true,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
      previousDependenciesGraph: initial.dependenciesGraph,
      previousResolvedPeerNamesByNodeId: resolvedPeerNamesByNodeId(initial),
    })
    return { cleaned, initial }
  }

  test('cleanup preserves an own locked peer across occurrences sharing a depPath', async () => {
    const xNodeId = '>retainer/1.0.0>x/1.0.0>' as NodeId
    const shallowSharedNodeId = '>shared/1.0.0>' as NodeId
    const propagatorNodeId = '>shared/1.0.0>propagator/1.0.0>' as NodeId
    const deepSharedNodeId = '>wrapper/1.0.0>shared/1.0.0>' as NodeId
    const xPeerDependencies: PeerDependencies = {
      x: { version: '1.0.0', optional: true },
    }
    const sharedPkg = fixturePkg('shared', xPeerDependencies)
    const resolutionOpts = options(
      new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [retainerNodeId, fixtureNode(retainerPkg, { children: { x: xNodeId } })],
        [xNodeId, fixtureNode('x', { depth: 1, previousDepPath: 'x/1.0.0' as DepPath })],
        [shallowSharedNodeId, fixtureNode(sharedPkg, { children: { propagator: propagatorNodeId } })],
        [propagatorNodeId, fixtureNode('propagator', {
          peerDependencies: xPeerDependencies,
          depth: 1,
          lockedPeerContext: { x: 'x/1.0.0' as DepPath },
        })],
        [wrapperNodeId, fixtureNode(wrapperPkg, { children: { shared: deepSharedNodeId } })],
        [deepSharedNodeId, fixtureNode(sharedPkg, { depth: 1, lockedPeerContext: { x: 'x/1.0.0' as DepPath } })],
      ]),
      new Map([
        ['retainer', retainerNodeId],
        ['shared', shallowSharedNodeId],
        ['wrapper', wrapperNodeId],
      ])
    )
    resolutionOpts.allPeerDepNames = new Set(['x'])
    const { initial, cleaned } = await resolveWithCleanup(resolutionOpts)
    const sharedWithX = 'shared/1.0.0(x/1.0.0)' as DepPath

    expect(initial.pathsByNodeId.get(shallowSharedNodeId)).toBe('shared/1.0.0')
    expect(initial.pathsByNodeId.get(deepSharedNodeId)).toBe('shared/1.0.0')
    expect(cleaned.pathsByNodeId.get(shallowSharedNodeId)).toBe(sharedWithX)
    expect(cleaned.pathsByNodeId.get(deepSharedNodeId)).toBe(sharedWithX)
    expect(cleaned.dependenciesGraph[sharedWithX].children).toEqual({
      propagator: 'propagator/1.0.0(x/1.0.0)',
    })
  })

  test('cleanup preserves an own peer below a cache-pruned survivor after injected dedupe', async () => {
    const ownerDepNodeId = '>retainer/1.0.0>dep/1.0.0>' as NodeId
    const survivingRetainerNodeId = '>wrapper/1.0.0>retainer/1.0.0>' as NodeId
    const survivingPeerNodeId = '>wrapper/1.0.0>retainer/1.0.0>peer/2.0.0>' as NodeId
    const survivingDepNodeId = '>wrapper/1.0.0>retainer/1.0.0>dep/1.0.0>' as NodeId
    const lazyChildren = jest.fn<() => ChildrenMap>(() => ({
      peer: survivingPeerNodeId,
      dep: survivingDepNodeId,
    }))
    const injectedRetainerPkg = {
      ...retainerPkg,
      id: 'file:workspace' as PkgResolutionId,
    }
    const depPkg = fixturePkg('dep', {
      peer: { version: '>=1' },
    })
    const directNodeIdsByAlias = new Map([
      ['injected', retainerNodeId],
      ['wrapper', wrapperNodeId],
    ])
    const resolutionOpts = options(
      new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [retainerNodeId, fixtureNode(injectedRetainerPkg, {
          children: { peer: retainedPeerNodeId, dep: ownerDepNodeId },
        })],
        [retainedPeerNodeId, fixtureNode(peer2Pkg, { depth: 1 })],
        [ownerDepNodeId, fixtureNode(depPkg, { depth: 1 })],
        [wrapperNodeId, fixtureNode(wrapperPkg, { children: { retainer: survivingRetainerNodeId } })],
        [survivingRetainerNodeId, fixtureNode(injectedRetainerPkg, { children: lazyChildren, depth: 1 })],
        [survivingPeerNodeId, fixtureNode(peer2Pkg, { depth: 2 })],
        [survivingDepNodeId, fixtureNode(depPkg, { depth: 2 })],
      ]),
      directNodeIdsByAlias
    )
    const { cleaned } = await resolveWithCleanup({
      ...resolutionOpts,
      dedupeInjectedDeps: true,
      resolvedImporters: {
        '': {
          directDependencies: [{
            alias: 'injected',
            pkgId: 'file:workspace' as PkgResolutionId,
          } as Parameters<typeof resolvePeers>[0]['resolvedImporters'][string]['directDependencies'][number]],
          directNodeIdsByAlias,
          hoistedPeerProviderNodeIds: new Set<NodeId>(),
          linkedDependencies: [],
        },
      },
      workspaceProjectIds: new Set(['workspace']),
    })
    const depWithPeer = 'dep/1.0.0(peer/2.0.0)' as DepPath

    expect(cleaned.dependenciesByProjectId[''].has('injected')).toBe(false)
    expect(lazyChildren).not.toHaveBeenCalled()
    expect(cleaned.dependenciesGraph['retainer/1.0.0' as DepPath].children.dep).toBe(depWithPeer)
    expect(cleaned.dependenciesGraph[depWithPeer].resolvedPeerNames).toEqual(new Set(['peer']))
  })

  test('cleanup accepts equivalent transitive provider contexts with dedupePeers', async () => {
    const xPkg = fixturePkg('x')
    const aPkg = fixturePkg('a', {
      s: { version: '1.0.0', optional: true },
    })
    const bPkg = fixturePkg('b', {
      t: { version: '1.0.0' },
    })
    const tPkg = fixturePkg('t', {
      u: { version: '*' },
    })
    const plain = createContext('plain', '1.0.0')
    const locked = createContext('locked', '2.0.0', true)
    const sNodeId = '>plain>s>' as NodeId
    plain.directNodeIdsByAlias.set('s', sNodeId)
    const resolutionOpts = options(
      new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        ...plain.nodes,
        ...locked.nodes,
        [sNodeId, fixtureNode('s', { previousDepPath: 's/1.0.0' as DepPath })],
      ]),
      plain.directNodeIdsByAlias
    )
    resolutionOpts.allPeerDepNames = new Set(['s', 't', 'u'])
    resolutionOpts.projects[0].id = 'plain'
    resolutionOpts.projects.push({
      ...resolutionOpts.projects[0],
      directNodeIdsByAlias: locked.directNodeIdsByAlias,
      id: 'locked',
    })
    const { initial, cleaned } = await resolveWithCleanup({
      ...resolutionOpts,
      dedupePeers: true,
    })
    const xWithT = 'x/1.0.0(t@1.0.0)' as DepPath
    const staleX = 'x/1.0.0(s@1.0.0)(t@1.0.0)' as DepPath

    expect(initial.pathsByNodeId.get(plain.tNodeId))
      .not.toBe(initial.pathsByNodeId.get(locked.tNodeId))
    expect(initial.pathsByNodeId.get(locked.innerXNodeId)).toBe(xWithT)
    expect(cleaned.pathsByNodeId.get(locked.innerXNodeId)).toBe(xWithT)
    expect(cleaned.dependenciesGraph[staleX]).toBeUndefined()

    function createContext (id: string, uVersion: string, reuseS = false) {
      const outerXNodeId = `>${id}>x>` as NodeId
      const innerXNodeId = `>${id}>x>x>` as NodeId
      const aNodeId = `>${id}>x>x>a>` as NodeId
      const bNodeId = `>${id}>x>x>b>` as NodeId
      const tNodeId = `>${id}>x>t>` as NodeId
      const uNodeId = `>${id}>u>` as NodeId
      const nodes: Array<[NodeId, DependenciesTreeNode<PartialResolvedPackage>]> = [
        [outerXNodeId, fixtureNode(xPkg, { children: { x: innerXNodeId, t: tNodeId } })],
        [innerXNodeId, fixtureNode(xPkg, {
          children: reuseS ? { a: aNodeId, b: bNodeId } : { b: bNodeId },
          depth: 1,
        })],
        [bNodeId, fixtureNode(bPkg, { depth: 2 })],
        [tNodeId, fixtureNode(tPkg, { depth: 1 })],
        [uNodeId, fixtureNode({
          ...fixturePkg('u'),
          pkgIdWithPatchHash: `u/${uVersion}` as PkgIdWithPatchHash,
          version: uVersion,
        })],
      ]
      if (reuseS) {
        nodes.push([aNodeId, fixtureNode(aPkg, { depth: 2, lockedPeerContext: { s: 's/1.0.0' as DepPath } })])
      }
      return {
        directNodeIdsByAlias: new Map([
          ['x', outerXNodeId],
          ['u', uNodeId],
        ]),
        innerXNodeId,
        nodes,
        tNodeId,
      }
    }
  })

  test('cleanup preserves a fresh peer depPath when its provider is deduped', async () => {
    const nNodeId = '>n>' as NodeId
    const triggerNodeId = '>n>trigger>' as NodeId
    const plainProvNodeId = '>prov>' as NodeId
    const provWithOptNodeId = '>with-opt>prov>' as NodeId
    const optNodeId = '>with-opt>opt>' as NodeId
    const spurNodeId = '>with-opt>spur>' as NodeId
    const provPkg = fixturePkg('prov', {
      opt: { version: '1.0.0', optional: true },
    })
    const resolutionOpts = options(
      new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [nNodeId, fixtureNode('n', { peerDependencies: { prov: { version: '1.0.0' } }, children: { trigger: triggerNodeId } })],
        [triggerNodeId, fixtureNode('trigger', {
          peerDependencies: { spur: { version: '1.0.0', optional: true } },
          depth: 1,
          lockedPeerContext: { spur: 'spur/1.0.0' as DepPath },
        })],
        [plainProvNodeId, fixtureNode(provPkg)],
        [provWithOptNodeId, fixtureNode(provPkg)],
        [optNodeId, fixtureNode('opt')],
        [spurNodeId, fixtureNode('spur', { previousDepPath: 'spur/1.0.0' as DepPath })],
      ]),
      new Map([
        ['n', nNodeId],
        ['prov', plainProvNodeId],
      ])
    )
    resolutionOpts.allPeerDepNames = new Set(['opt', 'prov', 'spur'])
    resolutionOpts.projects[0].id = 'plain'
    resolutionOpts.projects.push({
      ...resolutionOpts.projects[0],
      directNodeIdsByAlias: new Map([
        ['prov', provWithOptNodeId],
        ['opt', optNodeId],
        ['spur', spurNodeId],
      ]),
      id: 'with-opt',
    })
    const { initial, cleaned } = await resolveWithCleanup(resolutionOpts)
    const freshNDepPath = 'n/1.0.0(prov/1.0.0)' as DepPath

    expect(initial.dependenciesByProjectId.plain.get('n')).toBe(freshNDepPath)
    expect(cleaned.dependenciesByProjectId.plain.get('n')).toBe(freshNDepPath)
    expect(cleaned.dependenciesGraph[freshNDepPath].children).toEqual({
      trigger: 'trigger/1.0.0(spur/1.0.0)',
      prov: 'prov/1.0.0(opt/1.0.0)',
    })
  })

  test('cleanup follows a deduped provider referenced only by a peer hash', async () => {
    const nNodeId = '>n>' as NodeId
    const plainProvNodeId = '>prov>' as NodeId
    const plainDepNodeId = '>prov>dep>' as NodeId
    const qProvNodeId = '>with-q>prov>' as NodeId
    const qDepNodeId = '>with-q>prov>dep>' as NodeId
    const qNodeId = '>with-q>q>' as NodeId
    const retainerNodeId = '>retainer>' as NodeId
    const pNodeId = '>retainer>p>' as NodeId
    const provPkg = fixturePkg('prov', {
      q: { version: '1.0.0', optional: true },
    })
    const depPkg = fixturePkg('dep', {
      p: { version: '1.0.0', optional: true },
    })
    const resolutionOpts = options(
      new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [nNodeId, fixtureNode('n', { peerDependencies: { prov: { version: '1.0.0' } } })],
        [plainProvNodeId, fixtureNode(provPkg, { children: { dep: plainDepNodeId } })],
        [plainDepNodeId, fixtureNode(depPkg, { depth: 1, lockedPeerContext: { p: 'p/1.0.0' as DepPath } })],
        [qProvNodeId, fixtureNode(provPkg, { children: { dep: qDepNodeId } })],
        [qDepNodeId, fixtureNode(depPkg, { depth: 1, lockedPeerContext: { p: 'p/1.0.0' as DepPath } })],
        [qNodeId, fixtureNode('q')],
        [retainerNodeId, fixtureNode('retainer', { children: { p: pNodeId } })],
        [pNodeId, fixtureNode('p', { depth: 1, previousDepPath: 'p/1.0.0' as DepPath })],
      ]),
      new Map([
        ['n', nNodeId],
        ['prov', plainProvNodeId],
        ['retainer', retainerNodeId],
      ])
    )
    resolutionOpts.allPeerDepNames = new Set(['p', 'prov', 'q'])
    resolutionOpts.projects[0].id = 'plain'
    resolutionOpts.projects.push({
      ...resolutionOpts.projects[0],
      directNodeIdsByAlias: new Map([
        ['prov', qProvNodeId],
        ['q', qNodeId],
      ]),
      id: 'with-q',
    })
    const { initial, cleaned } = await resolveWithCleanup(resolutionOpts)
    const freshNDepPath = 'n/1.0.0(prov/1.0.0)' as DepPath

    expect(initial.dependenciesByProjectId.plain.get('n')).toBe(freshNDepPath)
    expect(cleaned.dependenciesByProjectId.plain.get('n')).toBe(freshNDepPath)
    expect(cleaned.dependenciesGraph[freshNDepPath].children.prov).toBe('prov/1.0.0(q/1.0.0)')
  })

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

  test('drops a spuriously reused optional peer that the fresh pass left unresolved', async () => {
    // Regression test for https://github.com/pnpm/pnpm/issues/12756. `dep` has an
    // OPTIONAL peer whose only provider (peer@2.0.0) lives in an unrelated sibling
    // subtree (under retainer), so a fresh resolution leaves it unbound. The
    // locked-context reuse pass still binds it under `consumer`'s `dep` — that
    // propagation is intentionally preserved so a shared provider can deduplicate.
    // It bubbles `peer` up onto `consumer`, whose own optional peer stayed unresolved.
    // The dropSpuriousReusePeers cleanup then removes that bubbled-up suffix from
    // `consumer` because the fresh pass did not resolve `peer` for it, keeping a
    // writable install aligned with `pnpm dedupe`.
    const depNodeId = '>wrapper/1.0.0>consumer/1.0.0>dep/1.0.0>' as NodeId
    const depPkg = fixturePkg('dep', {
      peer: { version: '>=1', optional: true },
    })
    const consumerWithUnresolvedPeerPkg = {
      ...consumerPkg,
      peerDependencies: {
        peer: { version: '>=1', optional: true },
      },
    }
    const dependenciesTree = new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      [retainerNodeId, fixtureNode(retainerPkg, { children: { peer: retainedPeerNodeId } })],
      [retainedPeerNodeId, fixtureNode(peer2Pkg, { depth: 1, previousDepPath: 'peer/2.0.0' as DepPath })],
      [wrapperNodeId, fixtureNode(wrapperPkg, { children: { consumer: consumerNodeId } })],
      [consumerNodeId, fixtureNode(consumerWithUnresolvedPeerPkg, { children: { dep: depNodeId }, depth: 1 })],
      [depNodeId, fixtureNode(depPkg, { depth: 2, lockedPeerContext: { peer: 'peer/2.0.0' as DepPath } })],
    ])
    const resolutionOpts = options(dependenciesTree, new Map([
      ['retainer', retainerNodeId],
      ['wrapper', wrapperNodeId],
    ]))
    const initial = await resolvePeers({
      ...resolutionOpts,
      dedupePeerDependents: false,
    })

    // Without the fresh-pass oracle the reuse pass still propagates the peer,
    // bubbling the suffixed instance up onto consumer, which the cleanup removes.
    const propagated = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
    })
    expect(propagated.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeTruthy()

    const cleaned = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
      dedupePeerDependents: true,
      previousDependenciesGraph: initial.dependenciesGraph,
      previousResolvedPeerNamesByNodeId: resolvedPeerNamesByNodeId(initial),
    })
    const cleanedConsumer = cleaned.dependenciesGraph['consumer/1.0.0' as DepPath]
    expect(cleanedConsumer).toBeTruthy()
    expect(cleanedConsumer.modules).toBe(path.join('consumer+1.0.0', 'node_modules'))
    expect(cleanedConsumer.dir).toBe(path.join('consumer+1.0.0', 'node_modules', 'consumer'))
    expect(cleaned.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeUndefined()
  })

  test('keeps a genuine reused peer context that the fresh pass also resolved', async () => {
    // The consumer's peer is REQUIRED and a provider (peer@1.0.0) is in scope, so
    // the fresh pass resolves the peer and the reuse pass re-binds it to the
    // locked peer@2.0.0. Because the fresh pass also resolved `peer`, the cleanup
    // treats the context as genuine and keeps it — the context preservation the
    // reuse pass exists for is not undone.
    const resolutionOpts = options(createTree(), new Map([
      ['peer', currentPeerNodeId],
      ['retainer', retainerNodeId],
      ['wrapper', wrapperNodeId],
    ]))
    const initial = await resolvePeers({
      ...resolutionOpts,
      dedupePeerDependents: false,
    })
    const cleaned = await resolvePeers({
      ...resolutionOpts,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
      dedupePeerDependents: true,
      previousDependenciesGraph: initial.dependenciesGraph,
      previousResolvedPeerNamesByNodeId: resolvedPeerNamesByNodeId(initial),
    })

    expect(cleaned.dependenciesGraph['consumer/1.0.0(peer/2.0.0)' as DepPath]).toBeTruthy()
    expect(cleaned.dependenciesGraph['consumer/1.0.0(peer/1.0.0)' as DepPath]).toBeUndefined()
  })

  test('cleanup preserves a full peer depPath hashed after the provider broke a cycle elsewhere', async () => {
    const mainNodeId = '>main/1.0.0>' as NodeId
    const pluginNodeId = '>plugin/1.0.0>' as NodeId
    const observerNodeId = '>observer/1.0.0>' as NodeId
    const pNodeId = '>observer/1.0.0>p/1.0.0>' as NodeId
    const containerNodeId = '>container/1.0.0>' as NodeId
    const depNodeId = '>container/1.0.0>dep/1.0.0>' as NodeId
    const resolutionOpts = {
      ...baseResolveOpts(),
      allPeerDepNames: new Set(['main', 'plugin', 'p']),
      projects: [
        project('.', new Map([['main', mainNodeId], ['plugin', pluginNodeId]])),
        project('child', new Map([['observer', observerNodeId], ['container', containerNodeId]])),
      ],
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [mainNodeId, fixtureNode('main', { peerDependencies: { plugin: { version: '1.0.0' } } })],
        [pluginNodeId, fixtureNode('plugin', { peerDependencies: { main: { version: '1.0.0' } } })],
        [observerNodeId, fixtureNode('observer', { peerDependencies: { main: { version: '1.0.0' } }, children: { p: pNodeId } })],
        [pNodeId, fixtureNode('p', { depth: 1, previousDepPath: 'p/1.0.0' as DepPath })],
        [containerNodeId, fixtureNode('container', { children: { dep: depNodeId } })],
        [depNodeId, fixtureNode('dep', {
          peerDependencies: { p: { version: '1.0.0', optional: true } },
          depth: 1,
          lockedPeerContext: { p: 'p/1.0.0' as DepPath },
        })],
      ]),
      resolvePeersFromWorkspaceRoot: true,
    }
    const { initial, cleaned } = await resolveWithCleanup(resolutionOpts)
    const observerDepPath = 'observer/1.0.0(main/1.0.0(plugin@1.0.0))' as DepPath

    expect(initial.dependenciesByProjectId.child.get('observer')).toBe(observerDepPath)
    expect(cleaned.dependenciesByProjectId.child.get('observer')).toBe(observerDepPath)
    expect(cleaned.dependenciesGraph[observerDepPath]).toBeTruthy()
    expect(cleaned.dependenciesByProjectId.child.get('container')).toBe('container/1.0.0')
  })

  test('cleanup does not expand a peer cycle broken by the global fallback', async () => {
    const mainNodeId = '>main@1.0.0>' as NodeId
    const cyclePluginNodeId = '>plugin@1.0.0>' as NodeId
    const plainPluginNodeId = '>provider>plugin@1.0.0>' as NodeId
    const containerNodeId = '>container/1.0.0>' as NodeId
    const depNodeId = '>container/1.0.0>dep/1.0.0>' as NodeId
    const mainPkg = {
      ...fixturePkg('main', {
        plugin: { version: '1.0.0' },
      }),
      pkgIdWithPatchHash: 'main@1.0.0' as PkgIdWithPatchHash,
    }
    const pluginPkg = {
      ...fixturePkg('plugin', {
        main: { version: '1.0.0' },
      }),
      pkgIdWithPatchHash: 'plugin@1.0.0' as PkgIdWithPatchHash,
    }
    const { initial, cleaned } = await resolveWithCleanup({
      ...baseResolveOpts(),
      allPeerDepNames: new Set(['main', 'plugin']),
      projects: [
        project('.', new Map([['main', mainNodeId], ['plugin', cyclePluginNodeId]]), {
          hoistedPeerProviderNodeIds: new Set([cyclePluginNodeId]),
        }),
        project('provider', new Map([['plugin', plainPluginNodeId]])),
        project('consumer', new Map([['container', containerNodeId]])),
      ],
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [mainNodeId, fixtureNode(mainPkg)],
        [cyclePluginNodeId, fixtureNode(pluginPkg, { depth: 1 })],
        [plainPluginNodeId, fixtureNode(pluginPkg, { previousDepPath: 'plugin@1.0.0' as DepPath })],
        [containerNodeId, fixtureNode('container', { children: { dep: depNodeId } })],
        [depNodeId, fixtureNode('dep', {
          peerDependencies: { plugin: { version: '1.0.0', optional: true } },
          depth: 1,
          lockedPeerContext: { plugin: 'plugin@1.0.0' as DepPath },
        })],
      ]),
    })
    const mainDepPath = 'main@1.0.0(plugin@1.0.0)' as DepPath

    expect(initial.dependenciesByProjectId['.'].get('main')).toBe(mainDepPath)
    expect(cleaned.dependenciesByProjectId['.'].get('main')).toBe(mainDepPath)
  })

  test('cleanup preserves cyclic peer hash provenance when a non-cycle peer is remapped', async () => {
    const pNodeId = '>p>' as NodeId
    const plainFNodeId = '>p>f>' as NodeId
    const qNodeId = '>p>q>' as NodeId
    const qDepNodeId = '>p>q>dep>' as NodeId
    const eParentNodeId = '>p>x>' as NodeId
    const eWithFNodeId = '>p>x>e>' as NodeId
    const consumerNodeId = '>b>' as NodeId
    const plainENodeId = '>b>e>' as NodeId
    const fParentNodeId = '>b>y>' as NodeId
    const fWithENodeId = '>b>y>f>' as NodeId
    const sNodeId = '>b>s>' as NodeId
    const ePkg = fixturePkg('e', {
      f: { version: '1.0.0', optional: true },
      q: { version: '1.0.0', optional: true },
    })
    const fPkg = fixturePkg('f', {
      e: { version: '1.0.0', optional: true },
    })
    const { cleaned } = await resolveWithCleanup({
      ...baseResolveOpts(),
      allPeerDepNames: new Set(['e', 'f', 'p', 'q', 's']),
      projects: [
        project('p', new Map([['p', pNodeId]])),
        project('b', new Map([['b', consumerNodeId], ['s', sNodeId]])),
      ],
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [pNodeId, fixtureNode('p', {
          children: { f: plainFNodeId, q: qNodeId, x: eParentNodeId },
          previousDepPath: 'p/1.0.0' as DepPath,
        })],
        [plainFNodeId, fixtureNode(fPkg, { depth: 1 })],
        [qNodeId, fixtureNode('q', { children: { dep: qDepNodeId }, depth: 1 })],
        [qDepNodeId, fixtureNode('dep', {
          peerDependencies: { s: { version: '1.0.0', optional: true } },
          depth: 2,
          lockedPeerContext: { s: 's/1.0.0' as DepPath },
        })],
        [eParentNodeId, fixtureNode('x', { children: { e: eWithFNodeId }, depth: 1 })],
        [eWithFNodeId, fixtureNode(ePkg, { depth: 2 })],
        [consumerNodeId, fixtureNode('b', { children: { e: plainENodeId, y: fParentNodeId } })],
        [sNodeId, fixtureNode('s', { previousDepPath: 's/1.0.0' as DepPath })],
        [plainENodeId, fixtureNode(ePkg, { depth: 1 })],
        [fParentNodeId, fixtureNode('y', {
          peerDependencies: { p: { version: '1.0.0', optional: true } },
          children: { f: fWithENodeId },
          depth: 1,
          lockedPeerContext: { p: 'p/1.0.0' as DepPath },
        })],
        [fWithENodeId, fixtureNode(fPkg, { depth: 2 })],
      ]),
    })

    const qWithS = 'q/1.0.0(s/1.0.0)' as DepPath
    const frozenE = 'e/1.0.0(f/1.0.0)(q/1.0.0(s/1.0.0))' as DepPath
    const eNode = cleaned.dependenciesGraph[frozenE]
    const peerIds = [...eNode.resolvedPeerIds!.values()].map((resolvedPeerId) =>
      'depPath' in resolvedPeerId ? resolvedPeerId.depPath : resolvedPeerId.peerId
    )

    expect(cleaned.pathsByNodeId.get(qNodeId)).toBe('q/1.0.0')
    expect(cleaned.pathsByNodeId.get(eWithFNodeId)).toBe(frozenE)
    expect(cleaned.pathsByNodeId.get(fWithENodeId)).toBe('f/1.0.0(e/1.0.0)')
    expect(eNode.resolvedPeerIds!.get('q')).toEqual({ depPath: qWithS, nodeId: qNodeId })
    expect(eNode.pkgIdWithPatchHash + createPeerDepGraphHash(peerIds)).toBe(frozenE)
  })

  test('cleanup preserves metadata and nested edges across a partial dedupe', async () => {
    const fooOneNodeId = '>one>foo/1.0.0>' as NodeId
    const depOneNodeId = '>one>foo/1.0.0>dep/1.0.0>' as NodeId
    const pNodeId = '>one>p/1.0.0>' as NodeId
    const wrapperNodeId = '>one>wrapper/1.0.0>' as NodeId
    const fooTwoNodeId = '>two>foo/1.0.0>' as NodeId
    const depTwoNodeId = '>two>foo/1.0.0>dep/1.0.0>' as NodeId
    const containerNodeId = '>two>foo/1.0.0>container/1.0.0>' as NodeId
    const qNodeId = '>two>q/1.0.0>' as NodeId
    const fooThreeNodeId = '>three>foo/1.0.0>' as NodeId
    const depThreeNodeId = '>three>foo/1.0.0>dep/1.0.0>' as NodeId
    const pThreeNodeId = '>three>p/1.0.0>' as NodeId
    const rNodeId = '>three>r/1.0.0>' as NodeId
    const fooPkg = fixturePkg('foo', {
      q: { version: '1.0.0', optional: true },
      r: { version: '1.0.0', optional: true },
    })
    const depPkg = fixturePkg('dep', {
      p: { version: '1.0.0', optional: true },
    })
    const resolutionOpts = {
      ...baseResolveOpts(),
      allPeerDepNames: new Set(['p', 'q', 'r']),
      projects: [
        project('one', new Map([['foo', fooOneNodeId], ['p', pNodeId], ['wrapper', wrapperNodeId]])),
        project('two', new Map([['foo', fooTwoNodeId], ['q', qNodeId]])),
        project('three', new Map([['foo', fooThreeNodeId], ['p', pThreeNodeId], ['r', rNodeId]])),
      ],
      dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [fooOneNodeId, fixtureNode(fooPkg, { children: { dep: depOneNodeId, container: containerNodeId } })],
        [depOneNodeId, fixtureNode(depPkg, { depth: 1 })],
        [pNodeId, fixtureNode('p', { previousDepPath: 'p/1.0.0' as DepPath })],
        [wrapperNodeId, fixtureNode('wrapper', { children: { foo: fooOneNodeId } })],
        [fooTwoNodeId, fixtureNode(fooPkg, { children: { dep: depTwoNodeId, container: containerNodeId }, installable: false })],
        [containerNodeId, fixtureNode('container', { children: { dep: depTwoNodeId }, depth: 1 })],
        [depTwoNodeId, fixtureNode(depPkg, { depth: 1, lockedPeerContext: { p: 'p/1.0.0' as DepPath } })],
        [qNodeId, fixtureNode('q')],
        [fooThreeNodeId, fixtureNode(fooPkg, { children: { dep: depThreeNodeId }, installable: false })],
        [depThreeNodeId, fixtureNode(depPkg, { depth: 1 })],
        [pThreeNodeId, fixtureNode('p', { previousDepPath: 'p/1.0.0' as DepPath })],
        [rNodeId, fixtureNode('r')],
      ]),
    }
    const { initial, cleaned } = await resolveWithCleanup(resolutionOpts)
    const fooWithP = 'foo/1.0.0(p/1.0.0)' as DepPath
    const dedupedFoo = 'foo/1.0.0(p/1.0.0)(q/1.0.0)' as DepPath
    const fooWithR = 'foo/1.0.0(p/1.0.0)(r/1.0.0)' as DepPath

    expect(initial.pathsByNodeId.get(fooOneNodeId)).toBe(fooWithP)
    expect(cleaned.dependenciesByProjectId.one.get('foo')).toBe(dedupedFoo)
    expect(cleaned.dependenciesByProjectId.two.get('foo')).toBe(dedupedFoo)
    expect(cleaned.dependenciesByProjectId.three.get('foo')).toBe(fooWithR)
    const wrapperDepPath = cleaned.dependenciesByProjectId.one.get('wrapper')!
    expect(cleaned.dependenciesGraph[wrapperDepPath].children.foo).toBe(fooWithP)
    expect(cleaned.dependenciesGraph[dedupedFoo].installable).toBe(true)
    expect(cleaned.dependenciesGraph[dedupedFoo].resolvedPeerNames).toEqual(new Set(['p', 'q']))
    expect(cleaned.pathsByNodeId.get(fooOneNodeId)).toBe(fooWithP)
    expect(cleaned.pathsByNodeId.get(fooTwoNodeId)).toBe(dedupedFoo)
    for (const { children } of Object.values(cleaned.dependenciesGraph)) {
      for (const childDepPath of Object.values<DepPath>(children)) {
        expect(cleaned.dependenciesGraph[childDepPath]).toBeDefined()
      }
    }
  })

  test('cleanup retains an injected package referenced by a surviving peer hash', async () => {
    const pNodeId = '>p>' as NodeId
    const bNodeId = '>p>b>' as NodeId
    const aNodeId = '>p>b>a>' as NodeId
    const qNodeId = '>q>' as NodeId
    const consumerNodeId = '>consumer>' as NodeId
    const directNodeIdsByAlias = new Map([
      ['p', pNodeId],
      ['q', qNodeId],
      ['consumer', consumerNodeId],
    ])
    const resolutionOpts = options(
      new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
        [pNodeId, fixtureNode({ ...fixturePkg('p'), id: 'file:workspace' as PkgResolutionId }, { children: { b: bNodeId } })],
        [bNodeId, fixtureNode('b', { children: { a: aNodeId }, depth: 1 })],
        [aNodeId, fixtureNode('a', { peerDependencies: { q: { version: '1.0.0' } }, depth: 2 })],
        [qNodeId, fixtureNode('q')],
        [consumerNodeId, fixtureNode('consumer', { peerDependencies: { p: { version: '1.0.0' } } })],
      ]),
      directNodeIdsByAlias
    )
    resolutionOpts.allPeerDepNames = new Set(['p', 'q'])
    resolutionOpts.projects.push({
      ...resolutionOpts.projects[0],
      directNodeIdsByAlias: new Map([
        ['b', bNodeId],
        ['q', qNodeId],
      ]),
      id: 'workspace',
    })
    const { initial, cleaned } = await resolveWithCleanup({
      ...resolutionOpts,
      dedupePeers: true,
      dedupeInjectedDeps: true,
      resolvedImporters: {
        '': {
          directDependencies: [{
            alias: 'p',
            pkgId: 'file:workspace' as PkgResolutionId,
          } as Parameters<typeof resolvePeers>[0]['resolvedImporters'][string]['directDependencies'][number]],
          directNodeIdsByAlias,
          hoistedPeerProviderNodeIds: new Set<NodeId>(),
          linkedDependencies: [],
        },
      },
      workspaceProjectIds: new Set(['workspace']),
    })
    const pWithQ = 'p/1.0.0(q@1.0.0)' as DepPath
    const bWithQ = 'b/1.0.0(q@1.0.0)' as DepPath
    const consumerWithP = 'consumer/1.0.0(p@1.0.0)' as DepPath
    const pPeerId = { peerId: { name: 'p', version: '1.0.0' }, nodeId: pNodeId }

    expect(initial.dependenciesByProjectId[''].has('p')).toBe(false)
    expect(initial.dependenciesGraph[consumerWithP].resolvedPeerIds!.get('p')).toEqual(pPeerId)
    expect(cleaned.dependenciesByProjectId[''].get('consumer')).toBe(consumerWithP)
    expect(cleaned.dependenciesGraph[consumerWithP].resolvedPeerIds!.get('p')).toEqual(pPeerId)
    expect(cleaned.dependenciesGraph[pWithQ].children.b).toBe(bWithQ)
    expect(cleaned.dependenciesGraph[bWithQ].resolvedPeerNames).toEqual(new Set(['q']))
    expect(cleaned.dependenciesGraph[pWithQ].resolvedPeerNames).toEqual(new Set(['q']))
  })

  test('cleanup ignores peer contexts from an injected subtree deduped away', async () => {
    const injectedNodeId = '>injected/1.0.0>' as NodeId
    const peerNodeId = '>injected/1.0.0>peer/1.0.0>' as NodeId
    const injectedConsumerNodeId = '>injected/1.0.0>consumer/1.0.0>' as NodeId
    const injectedDepNodeId = '>injected/1.0.0>consumer/1.0.0>dep/1.0.0>' as NodeId
    const survivingConsumerNodeId = '>consumer/1.0.0>' as NodeId
    const survivingDepNodeId = '>consumer/1.0.0>dep/1.0.0>' as NodeId
    const holderNodeId = '>holder/1.0.0>' as NodeId
    const consumerPkg = fixturePkg('consumer')
    const depPkg = fixturePkg('dep', {
      peer: { version: '1.0.0', optional: true },
    })
    const directNodeIdsByAlias = new Map([
      ['injected', injectedNodeId],
      ['consumer', survivingConsumerNodeId],
      ['holder', holderNodeId],
    ])
    const dependenciesTree = new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      [injectedNodeId, fixtureNode({ ...fixturePkg('injected'), id: 'file:workspace' as PkgResolutionId }, {
        children: { peer: peerNodeId, consumer: injectedConsumerNodeId },
      })],
      [peerNodeId, fixtureNode('peer', { depth: 1, previousDepPath: 'peer/1.0.0' as DepPath })],
      [injectedConsumerNodeId, fixtureNode(consumerPkg, { children: { dep: injectedDepNodeId }, depth: 1 })],
      [injectedDepNodeId, fixtureNode(depPkg, { depth: 2 })],
      [survivingConsumerNodeId, fixtureNode(consumerPkg, { children: { dep: survivingDepNodeId } })],
      [survivingDepNodeId, fixtureNode(depPkg, { depth: 1, lockedPeerContext: { peer: 'peer/1.0.0' as DepPath } })],
      [holderNodeId, fixtureNode('holder', {
        peerDependencies: {
          consumer: { version: '1.0.0' },
        },
      })],
    ])
    const resolutionOpts = {
      ...options(dependenciesTree, directNodeIdsByAlias),
      resolvedImporters: {
        '': {
          directDependencies: [{
            alias: 'injected',
            pkgId: 'file:workspace' as PkgResolutionId,
          } as Parameters<typeof resolvePeers>[0]['resolvedImporters'][string]['directDependencies'][number]],
          directNodeIdsByAlias,
          hoistedPeerProviderNodeIds: new Set<NodeId>(),
          linkedDependencies: [],
        },
      },
      workspaceProjectIds: new Set(['workspace']),
    }
    resolutionOpts.allPeerDepNames = new Set(['consumer', 'peer'])
    const initial = await resolvePeers({
      ...resolutionOpts,
      dedupeInjectedDeps: false,
      dedupePeerDependents: false,
    })
    const cleaned = await resolvePeers({
      ...resolutionOpts,
      dedupeInjectedDeps: true,
      dedupePeerDependents: true,
      resolvedPeerProviderPaths: initial.pathsByNodeId,
      previousDependenciesGraph: initial.dependenciesGraph,
      previousResolvedPeerNamesByNodeId: resolvedPeerNamesByNodeId(initial),
    })
    const consumerWithPeer = 'consumer/1.0.0(peer/1.0.0)' as DepPath
    const plainConsumer = 'consumer/1.0.0' as DepPath
    const holderWithConsumer = 'holder/1.0.0(consumer/1.0.0)' as DepPath

    expect(initial.pathsByNodeId.get(injectedConsumerNodeId)).toBe(consumerWithPeer)
    expect(initial.pathsByNodeId.get(survivingConsumerNodeId)).toBe(plainConsumer)
    expect(cleaned.dependenciesByProjectId['']).toEqual(new Map([
      ['consumer', plainConsumer],
      ['holder', holderWithConsumer],
    ]))
    expect(cleaned.dependenciesGraph[holderWithConsumer].resolvedPeerIds!.get('consumer')).toEqual({
      depPath: plainConsumer,
      nodeId: survivingConsumerNodeId,
    })
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

test('pruned hoisted peer providers that peer-depend on each other are resolved together', async () => {
  // Hoisted peer providers whose tree position was never visited are resolved
  // by a root-context fallback. Providers frequently peer-depend on each
  // other, and each resolvePeersOfChildren call only detects peer cycles
  // among its own children — so all pruned providers must be resolved in one
  // call, or their dep path calculations await each other forever.
  const libAPkg = {
    name: 'lib-a',
    pkgIdWithPatchHash: 'lib-a@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      'lib-b': { version: '^1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const libBPkg = {
    name: 'lib-b',
    pkgIdWithPatchHash: 'lib-b@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      'lib-a': { version: '^1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const consumerPkg = {
    name: 'consumer',
    pkgIdWithPatchHash: 'consumer@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      'lib-a': { version: '^1.0.0' },
      'lib-b': { version: '^1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const { dependenciesGraph, dependenciesByProjectId } = await resolvePeers({
    allPeerDepNames: new Set(['lib-a', 'lib-b']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['consumer', '>consumer@1.0.0>' as NodeId],
          ['lib-a', '>lib-a@1.0.0>' as NodeId],
          ['lib-b', '>lib-b@1.0.0>' as NodeId],
        ]),
        hoistedPeerProviderNodeIds: new Set(['>lib-a@1.0.0>' as NodeId, '>lib-b@1.0.0>' as NodeId]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: '.',
      },
    ],
    resolvedImporters: {},
    dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      ['>consumer@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: consumerPkg,
        depth: 0,
      }],
      ['>lib-a@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: libAPkg,
        depth: 1,
      }],
      ['>lib-b@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: libBPkg,
        depth: 1,
      }],
    ]),
    virtualStoreDir: '',
    lockfileDir: '',
    virtualStoreDirMaxLength: 120,
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })
  expect(Object.keys(dependenciesGraph).sort()).toStrictEqual([
    'consumer@1.0.0(lib-a@1.0.0)(lib-b@1.0.0)',
    'lib-a@1.0.0(lib-b@1.0.0)',
    'lib-b@1.0.0(lib-a@1.0.0)',
  ])
  expect(dependenciesByProjectId['.'].get('lib-a')).toBe('lib-a@1.0.0(lib-b@1.0.0)')
  expect(dependenciesByProjectId['.'].get('lib-b')).toBe('lib-b@1.0.0(lib-a@1.0.0)')
})

test('a pruned hoisted peer provider is resolved by the root-context fallback', async () => {
  // Mirror of the pacquet test `pruned_hoisted_provider_falls_back_to_root_resolution`:
  // a hoisted peer provider whose tree position was never visited must still
  // get a dep path so the consumers that bound it can finish.
  const provPkg = {
    name: 'prov',
    pkgIdWithPatchHash: 'prov@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {} as PeerDependencies,
    id: '' as PkgResolutionId,
  }
  const consumerPkg = {
    name: 'consumer',
    pkgIdWithPatchHash: 'consumer@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      prov: { version: '*' },
    },
    id: '' as PkgResolutionId,
  }
  const { dependenciesGraph, dependenciesByProjectId } = await resolvePeers({
    allPeerDepNames: new Set(['prov']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['consumer', '>consumer@1.0.0>' as NodeId],
          ['prov', '>prov@1.0.0>' as NodeId],
        ]),
        hoistedPeerProviderNodeIds: new Set(['>prov@1.0.0>' as NodeId]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: '.',
      },
    ],
    resolvedImporters: {},
    dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      ['>consumer@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: consumerPkg,
        depth: 0,
      }],
      ['>prov@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: provPkg,
        depth: 1,
      }],
    ]),
    virtualStoreDir: '',
    lockfileDir: '',
    virtualStoreDirMaxLength: 120,
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })
  expect(dependenciesByProjectId['.'].get('prov')).toBe('prov@1.0.0')
  expect(Object.keys(dependenciesGraph)).toContain('consumer@1.0.0(prov@1.0.0)')
})

test('an own direct dependency and a pruned hoisted peer provider that peer-depend on each other are resolved together', async () => {
  // Regression test for https://github.com/pnpm/pnpm/issues/12921: the peer
  // cycle spans two resolvePeersOfChildren calls (the own direct children and
  // the pruned-provider fallback), so neither call's cycle analysis sees it —
  // the dep path calculations awaited each other forever.
  const mainPkg = {
    name: 'main',
    pkgIdWithPatchHash: 'main@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      plugin: { version: '^1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const pluginPkg = {
    name: 'plugin',
    pkgIdWithPatchHash: 'plugin@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      main: { version: '^1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const { dependenciesGraph, dependenciesByProjectId } = await resolvePeers({
    allPeerDepNames: new Set(['main', 'plugin']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['main', '>main@1.0.0>' as NodeId],
          ['plugin', '>plugin@1.0.0>' as NodeId],
        ]),
        hoistedPeerProviderNodeIds: new Set(['>plugin@1.0.0>' as NodeId]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: '.',
      },
    ],
    resolvedImporters: {},
    dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      ['>main@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: mainPkg,
        depth: 0,
      }],
      ['>plugin@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: pluginPkg,
        depth: 1,
      }],
    ]),
    virtualStoreDir: '',
    lockfileDir: '',
    virtualStoreDirMaxLength: 120,
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })
  expect(Object.keys(dependenciesGraph).sort()).toStrictEqual([
    'main@1.0.0(plugin@1.0.0)',
    'plugin@1.0.0(main@1.0.0)',
  ])
  expect(dependenciesByProjectId['.'].get('main')).toBe('main@1.0.0(plugin@1.0.0)')
  expect(dependenciesByProjectId['.'].get('plugin')).toBe('plugin@1.0.0(main@1.0.0)')
})

test('a peer cycle between an own direct dependency and a hoisted peer provider resolved at its tree position does not deadlock', async () => {
  // Same await cycle as in https://github.com/pnpm/pnpm/issues/12921, but the
  // provider is visited at its true tree position (inside host's subtree), so
  // the cycle spans two traversal levels instead of two root-level calls.
  const hostPkg = {
    name: 'host',
    pkgIdWithPatchHash: 'host@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {} as PeerDependencies,
    id: '' as PkgResolutionId,
  }
  const mainPkg = {
    name: 'main',
    pkgIdWithPatchHash: 'main@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      plugin: { version: '^1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const pluginPkg = {
    name: 'plugin',
    pkgIdWithPatchHash: 'plugin@1.0.0' as PkgIdWithPatchHash,
    version: '1.0.0',
    peerDependencies: {
      main: { version: '^1.0.0' },
    },
    id: '' as PkgResolutionId,
  }
  const { dependenciesGraph, dependenciesByProjectId } = await resolvePeers({
    allPeerDepNames: new Set(['main', 'plugin']),
    projects: [
      {
        directNodeIdsByAlias: new Map([
          ['host', '>host@1.0.0>' as NodeId],
          ['main', '>main@1.0.0>' as NodeId],
          ['plugin', '>host@1.0.0>plugin@1.0.0>' as NodeId],
        ]),
        hoistedPeerProviderNodeIds: new Set(['>host@1.0.0>plugin@1.0.0>' as NodeId]),
        topParents: [],
        rootDir: '' as ProjectRootDir,
        id: '.',
      },
    ],
    resolvedImporters: {},
    dependenciesTree: new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      ['>host@1.0.0>' as NodeId, {
        children: { plugin: '>host@1.0.0>plugin@1.0.0>' as NodeId },
        installable: true,
        resolvedPackage: hostPkg,
        depth: 0,
      }],
      ['>main@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: mainPkg,
        depth: 0,
      }],
      ['>host@1.0.0>plugin@1.0.0>' as NodeId, {
        children: {},
        installable: true,
        resolvedPackage: pluginPkg,
        depth: 1,
      }],
    ]),
    virtualStoreDir: '',
    lockfileDir: '',
    virtualStoreDirMaxLength: 120,
    peersSuffixMaxLength: 1000,
    workspaceProjectIds: new Set(),
  })
  expect(Object.keys(dependenciesGraph).sort()).toStrictEqual([
    'host@1.0.0(main@1.0.0)',
    'main@1.0.0(plugin@1.0.0)',
    'plugin@1.0.0(main@1.0.0)',
  ])
  expect(dependenciesByProjectId['.'].get('host')).toBe('host@1.0.0(main@1.0.0)')
  expect(dependenciesByProjectId['.'].get('main')).toBe('main@1.0.0(plugin@1.0.0)')
  expect(dependenciesByProjectId['.'].get('plugin')).toBe('plugin@1.0.0(main@1.0.0)')
})
