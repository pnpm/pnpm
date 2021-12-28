/// <reference path="../../../typings/index.d.ts" />
import resolvePeers from '@pnpm/resolve-dependencies/lib/resolvePeers'

test('resolve peer dependencies of cyclic dependencies', () => {
  const fooPkg = {
    name: 'foo',
    depPath: 'foo/1.0.0',
    version: '1.0.0',
    peerDependencies: {
      qar: '1.0.0',
      zoo: '1.0.0',
    },
  }
  const barPkg = {
    name: 'bar',
    depPath: 'bar/1.0.0',
    version: '1.0.0',
    peerDependencies: {
      foo: '1.0.0',
      zoo: '1.0.0',
    } as Record<string, string>,
  }
  const { dependenciesGraph } = resolvePeers({
    projects: [
      {
        directNodeIdsByAlias: {
          foo: '>foo/1.0.0>',
        },
        topParents: [],
        rootDir: '',
        id: '',
      },
    ],
    dependenciesTree: {
      '>foo/1.0.0>': {
        children: {
          bar: '>foo/1.0.0>bar/1.0.0>',
        },
        installable: true,
        resolvedPackage: fooPkg,
        depth: 0,
      },
      '>foo/1.0.0>bar/1.0.0>': {
        children: {
          qar: '>foo/1.0.0>bar/1.0.0>qar/1.0.0>',
        },
        installable: true,
        resolvedPackage: barPkg,
        depth: 1,
      },
      '>foo/1.0.0>bar/1.0.0>qar/1.0.0>': {
        children: {
          zoo: '>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>',
        },
        installable: true,
        resolvedPackage: {
          name: 'qar',
          depPath: 'qar/1.0.0',
          version: '1.0.0',
          peerDependencies: {
            foo: '1.0.0',
            bar: '1.0.0',
          },
        },
        depth: 2,
      },
      '>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>': {
        children: {
          foo: '>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>foo/1.0.0>',
          bar: '>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>bar/1.0.0>',
        },
        installable: true,
        resolvedPackage: {
          name: 'zoo',
          depPath: 'zoo/1.0.0',
          version: '1.0.0',
          peerDependencies: {
            qar: '1.0.0',
          },
        },
        depth: 3,
      },
      '>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>foo/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 4,
      },
      '>foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>bar/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: barPkg,
        depth: 4,
      },
    },
    virtualStoreDir: '',
    lockfileDir: '',
  })
  expect(Object.keys(dependenciesGraph)).toStrictEqual([
    'foo/1.0.0_qar@1.0.0+zoo@1.0.0',
    'bar/1.0.0_foo@1.0.0+zoo@1.0.0',
    'zoo/1.0.0_qar@1.0.0',
    'qar/1.0.0_bar@1.0.0+foo@1.0.0',
    'bar/1.0.0_foo@1.0.0',
    'foo/1.0.0',
  ])
})

test('when a package is referenced twice in the dependencies graph and one of the times it cannot resolve its peers, still try to resolve it in the other occurence', () => {
  const fooPkg = {
    name: 'foo',
    depPath: 'foo/1.0.0',
    version: '1.0.0',
    peerDependencies: {
      qar: '1.0.0',
    },
  }
  const barPkg = {
    name: 'bar',
    depPath: 'bar/1.0.0',
    version: '1.0.0',
    peerDependencies: {} as Record<string, string>,
  }
  const zooPkg = {
    name: 'zoo',
    depPath: 'zoo/1.0.0',
    version: '1.0.0',
    peerDependencies: {} as Record<string, string>,
  }
  const { dependenciesGraph } = resolvePeers({
    projects: [
      {
        directNodeIdsByAlias: {
          zoo: '>zoo/1.0.0>',
          bar: '>bar/1.0.0>',
        },
        topParents: [],
        rootDir: '',
        id: '',
      },
    ],
    dependenciesTree: {
      '>zoo/1.0.0>': {
        children: {
          foo: '>zoo/1.0.0>foo/1.0.0>',
        },
        installable: true,
        resolvedPackage: zooPkg,
        depth: 0,
      },
      '>zoo/1.0.0>foo/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 1,
      },
      '>bar/1.0.0>': {
        children: {
          zoo: '>bar/1.0.0>zoo/1.0.0>',
          qar: '>bar/1.0.0>qar/1.0.0>',
        },
        installable: true,
        resolvedPackage: barPkg,
        depth: 0,
      },
      '>bar/1.0.0>zoo/1.0.0>': {
        children: {
          foo: '>bar/1.0.0>zoo/1.0.0>foo/1.0.0>',
        },
        installable: true,
        resolvedPackage: zooPkg,
        depth: 1,
      },
      '>bar/1.0.0>zoo/1.0.0>foo/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 2,
      },
      '>bar/1.0.0>qar/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: {
          name: 'qar',
          depPath: 'qar/1.0.0',
          version: '1.0.0',
          peerDependencies: {},
        },
        depth: 1,
      },
    },
    virtualStoreDir: '',
    lockfileDir: '',
  })
  expect(Object.keys(dependenciesGraph)).toStrictEqual([
    'foo/1.0.0',
    'zoo/1.0.0',
    'foo/1.0.0_qar@1.0.0',
    'zoo/1.0.0_qar@1.0.0',
    'qar/1.0.0',
    'bar/1.0.0',
  ])
})

describe('peer dependency issues', () => {
  const fooPkg = {
    name: 'foo',
    depPath: 'foo/1.0.0',
    version: '1.0.0',
    peerDependencies: {
      peer: '1',
    },
  }
  const fooWithOptionalPeer = {
    name: 'foo',
    depPath: 'foo/2.0.0',
    version: '2.0.0',
    peerDependencies: {
      peer: '1',
    },
    peerDependenciesMeta: {
      peer: {
        optional: true,
      },
    },
  }
  const barPkg = {
    name: 'bar',
    depPath: 'bar/1.0.0',
    version: '1.0.0',
    peerDependencies: {
      peer: '2',
    },
  }
  const barWithOptionalPeer = {
    name: 'bar',
    depPath: 'bar/2.0.0',
    version: '2.0.0',
    peerDependencies: {
      peer: '2',
    },
    peerDependenciesMeta: {
      peer: {
        optional: true,
      },
    },
  }
  const qarPkg = {
    name: 'qar',
    depPath: 'qar/1.0.0',
    version: '1.0.0',
    peerDependencies: {
      peer: '^2.2.0',
    },
  }
  const { peerDependencyIssuesByProjects } = resolvePeers({
    projects: [
      {
        directNodeIdsByAlias: {
          foo: '>project1>foo/1.0.0>',
        },
        topParents: [],
        rootDir: '',
        id: 'project1',
      },
      {
        directNodeIdsByAlias: {
          bar: '>project2>bar/1.0.0>',
        },
        topParents: [],
        rootDir: '',
        id: 'project2',
      },
      {
        directNodeIdsByAlias: {
          foo: '>project3>foo/1.0.0>',
          bar: '>project3>bar/1.0.0>',
        },
        topParents: [],
        rootDir: '',
        id: 'project3',
      },
      {
        directNodeIdsByAlias: {
          bar: '>project4>bar/1.0.0>',
          qar: '>project4>qar/1.0.0>',
        },
        topParents: [],
        rootDir: '',
        id: 'project4',
      },
      {
        directNodeIdsByAlias: {
          foo: '>project5>foo/1.0.0>',
          bar: '>project5>bar/2.0.0>',
        },
        topParents: [],
        rootDir: '',
        id: 'project5',
      },
      {
        directNodeIdsByAlias: {
          foo: '>project6>foo/2.0.0>',
          bar: '>project6>bar/2.0.0>',
        },
        topParents: [],
        rootDir: '',
        id: 'project6',
      },
    ],
    dependenciesTree: {
      '>project1>foo/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 0,
      },
      '>project2>bar/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: barPkg,
        depth: 0,
      },
      '>project3>foo/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 0,
      },
      '>project3>bar/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: barPkg,
        depth: 0,
      },
      '>project4>bar/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: barPkg,
        depth: 0,
      },
      '>project4>qar/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: qarPkg,
        depth: 0,
      },
      '>project5>foo/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 0,
      },
      '>project5>bar/2.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: barWithOptionalPeer,
        depth: 0,
      },
      '>project6>foo/2.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: fooWithOptionalPeer,
        depth: 0,
      },
      '>project6>bar/2.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: barWithOptionalPeer,
        depth: 0,
      },
    },
    virtualStoreDir: '',
    lockfileDir: '',
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
      .toStrictEqual({ peer: '>=2.2.0 <3.0.0' })
  })
})

describe('unmet peer dependency issues', () => {
  const { peerDependencyIssuesByProjects } = resolvePeers({
    projects: [
      {
        directNodeIdsByAlias: {
          foo: '>project1>foo/1.0.0>',
          peer1: '>project1>peer1/1.0.0-rc.0>',
          peer2: '>project1>peer2/1.1.0-rc.0>',
        },
        topParents: [],
        rootDir: '',
        id: 'project1',
      },
    ],
    dependenciesTree: {
      '>project1>foo/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: {
          name: 'foo',
          version: '1.0.0',
          depPath: 'foo/1.0.0',
          peerDependencies: {
            peer1: '*',
            peer2: '>=1',
          },
        },
        depth: 0,
      },
      '>project1>peer1/1.0.0-rc.0>': {
        children: {},
        installable: true,
        resolvedPackage: {
          name: 'peer1',
          version: '1.0.0-rc.0',
          depPath: 'peer/1.0.0-rc.0',
          peerDependencies: {},
        },
        depth: 0,
      },
      '>project1>peer2/1.1.0-rc.0>': {
        children: {},
        installable: true,
        resolvedPackage: {
          name: 'peer2',
          version: '1.1.0-rc.0',
          depPath: 'peer/1.1.0-rc.0',
          peerDependencies: {},
        },
        depth: 0,
      },
    },
    virtualStoreDir: '',
    lockfileDir: '',
  })
  it('should not warn when the found package has prerelease version and the wanted range is *', () => {
    expect(peerDependencyIssuesByProjects).not.toHaveProperty(['project1', 'bad', 'peer1'])
  })
  it('should not warn when the found package is a prerelease version but satisfies the range', () => {
    expect(peerDependencyIssuesByProjects).not.toHaveProperty(['project1', 'bad', 'peer2'])
  })
})

describe('unmet peer dependency issue resolved from subdependency', () => {
  const { peerDependencyIssuesByProjects } = resolvePeers({
    projects: [
      {
        directNodeIdsByAlias: {
          foo: '>project>foo/1.0.0>',
        },
        topParents: [],
        rootDir: '',
        id: 'project',
      },
    ],
    dependenciesTree: {
      '>project>foo/1.0.0>': {
        children: {
          dep: '>project>foo/1.0.0>dep/1.0.0>',
          bar: '>project>foo/1.0.0>bar/1.0.0>',
        },
        installable: true,
        resolvedPackage: {
          name: 'foo',
          depPath: 'foo/1.0.0',
          version: '1.0.0',
          peerDependencies: {},
        },
        depth: 0,
      },
      '>project>foo/1.0.0>dep/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: {
          name: 'dep',
          depPath: 'dep/1.0.0',
          version: '1.0.0',
          peerDependencies: {},
        },
        depth: 1,
      },
      '>project>foo/1.0.0>bar/1.0.0>': {
        children: {},
        installable: true,
        resolvedPackage: {
          name: 'bar',
          depPath: 'bar/1.0.0',
          version: '1.0.0',
          peerDependencies: {
            dep: '10',
          },
        },
        depth: 1,
      },
    },
    virtualStoreDir: '',
    lockfileDir: '',
  })
  it('should return from where the bad peer dependency is resolved', () => {
    expect(peerDependencyIssuesByProjects.project.bad.dep[0].resolvedFrom).toStrictEqual([{ name: 'foo', version: '1.0.0' }])
  })
})
