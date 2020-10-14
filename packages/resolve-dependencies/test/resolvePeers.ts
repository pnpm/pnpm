import resolvePeers from '@pnpm/resolve-dependencies/lib/resolvePeers'
import test = require('tape')

test('resolve peer dependencies of cyclic dependencies', (t) => {
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
          foo: 'foo/1.0.0',
        },
        topParents: [],
        rootDir: '',
        id: '',
      },
    ],
    dependenciesTree: {
      'foo/1.0.0': {
        children: {
          bar: 'foo/1.0.0>bar/1.0.0',
        },
        installable: true,
        resolvedPackage: fooPkg,
        depth: 0,
      },
      'foo/1.0.0>bar/1.0.0': {
        children: {
          qar: 'foo/1.0.0>bar/1.0.0>qar/1.0.0',
        },
        installable: true,
        resolvedPackage: barPkg,
        depth: 1,
      },
      'foo/1.0.0>bar/1.0.0>qar/1.0.0': {
        children: {
          zoo: 'foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0',
        },
        installable: true,
        resolvedPackage: {
          name: 'zoo',
          depPath: 'zoo/1.0.0',
          version: '1.0.0',
          peerDependencies: {
            foo: '1.0.0',
            bar: '1.0.0',
          },
        },
        depth: 2,
      },
      'foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0': {
        children: {
          foo: 'foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>foo/1.0.0',
          bar: 'foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>bar/1.0.0',
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
      'foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>foo/1.0.0': {
        children: {},
        installable: true,
        resolvedPackage: fooPkg,
        depth: 4,
      },
      'foo/1.0.0>bar/1.0.0>qar/1.0.0>zoo/1.0.0>bar/1.0.0': {
        children: {},
        installable: true,
        resolvedPackage: barPkg,
        depth: 4,
      },
    },
    virtualStoreDir: '',
    lockfileDir: '',
    strictPeerDependencies: false,
  })
  t.deepEqual(Object.keys(dependenciesGraph), [
    'foo/1.0.0_qar@1.0.0+zoo@1.0.0',
    'bar/1.0.0_foo@1.0.0+zoo@1.0.0',
    'zoo/1.0.0_qar@1.0.0',
    'zoo/1.0.0_bar@1.0.0+foo@1.0.0+qar@1.0.0',
    'bar/1.0.0_foo@1.0.0',
    'foo/1.0.0',
  ])
  t.end()
})
