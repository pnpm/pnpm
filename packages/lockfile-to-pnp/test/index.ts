import { lockfileToPackageRegistry } from '../lib'

test('lockfileToPackageRegistry', () => {
  const packageRegistry = lockfileToPackageRegistry({
    importers: {
      importer1: {
        dependencies: {
          dep1: '1.0.0',
          dep2: '/foo/2.0.0',
        },
        optionalDependencies: {
          qar: '2.0.0',
        },
        specifiers: {},
      },
      importer2: {
        devDependencies: {
          importer1: 'link:../importer1',
        },
        specifiers: {},
      },
    },
    lockfileVersion: 5,
    packages: {
      '/dep1/1.0.0': {
        dependencies: {
          dep2: '/foo/2.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      '/foo/2.0.0': {
        dependencies: {
          qar: '3.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      '/qar/2.0.0': {
        resolution: {
          integrity: '',
        },
      },
      '/qar/3.0.0': {
        resolution: {
          integrity: '',
        },
      },
    },
  }, {
    importerNames: {
      importer1: 'importer1',
      importer2: 'importer2',
    },
    lockfileDirectory: '/home/zoli/src/proj',
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    virtualStoreDir: '/home/zoli/.pnpm-store/2',
  })

  const actual = Array
    .from(packageRegistry.entries())
    .map(([packageName, packageStoreMap]) => {
      return [
        packageName,
        Array.from(packageStoreMap.entries())
          .map(([pkgRef, packageInfo]: any) => { // eslint-disable-line
            return [
              pkgRef,
              {
                packageDependencies: Array.from(packageInfo.packageDependencies || new Map()),
                packageLocation: packageInfo.packageLocation,
              },
            ]
          }),
      ]
    })
  expect(actual).toStrictEqual([
    [
      'importer1',
      [
        [
          'importer1',
          {
            packageDependencies: [
              ['importer1', 'importer1'],
              ['dep1', '1.0.0'],
              ['dep2', ['foo', '2.0.0']],
              ['qar', '2.0.0'],
            ],
            packageLocation: 'importer1',
          },
        ],
      ],
    ],
    [
      'importer2',
      [
        [
          'importer2',
          {
            packageDependencies: [
              ['importer2', 'importer2'],
              ['importer1', 'importer1'],
            ],
            packageLocation: 'importer2',
          },
        ],
      ],
    ],
    [
      'dep1',
      [
        [
          '1.0.0',
          {
            packageDependencies: [
              ['dep1', '1.0.0'],
              ['dep2', ['foo', '2.0.0']],
            ],
            packageLocation: '/home/zoli/.pnpm-store/2/dep1@1.0.0/node_modules/dep1',
          },
        ],
      ],
    ],
    [
      'foo',
      [
        [
          '2.0.0',
          {
            packageDependencies: [
              ['foo', '2.0.0'],
              ['qar', '3.0.0'],
            ],
            packageLocation: '/home/zoli/.pnpm-store/2/foo@2.0.0/node_modules/foo',
          },
        ],
      ],
    ],
    [
      'qar',
      [
        [
          '2.0.0',
          {
            packageDependencies: [
              ['qar', '2.0.0'],
            ],
            packageLocation: '/home/zoli/.pnpm-store/2/qar@2.0.0/node_modules/qar',
          },
        ],
        [
          '3.0.0',
          {
            packageDependencies: [
              ['qar', '3.0.0'],
            ],
            packageLocation: '/home/zoli/.pnpm-store/2/qar@3.0.0/node_modules/qar',
          },
        ],
      ],
    ],
  ])
})

test('lockfileToPackageRegistry packages that have peer deps', () => {
  const packageRegistry = lockfileToPackageRegistry({
    importers: {
      importer: {
        dependencies: {
          haspeer: '2.0.0_peer@1.0.0',
          peer: '1.0.0',
        },
        specifiers: {},
      },
    },
    lockfileVersion: 5,
    packages: {
      '/haspeer/2.0.0_peer@1.0.0': {
        dependencies: {
          peer: '1.0.0',
        },
        peerDependencies: {
          peer: '^1.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      '/peer/1.0.0': {
        resolution: {
          integrity: '',
        },
      },
    },
  }, {
    importerNames: {
      importer: 'importer',
    },
    lockfileDirectory: '/home/zoli/src/proj',
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    virtualStoreDir: '/home/zoli/.pnpm-store/2',
  })

  const actual = Array
    .from(packageRegistry.entries())
    .map(([packageName, packageStoreMap]) => {
      return [
        packageName,
        Array.from(packageStoreMap.entries())
          .map(([pkgRef, packageInfo]: any) => { // eslint-disable-line
            return [
              pkgRef,
              {
                packageDependencies: Array.from(packageInfo.packageDependencies || new Map()),
                packageLocation: packageInfo.packageLocation,
              },
            ]
          }),
      ]
    })

  expect(actual).toStrictEqual([
    [
      'importer',
      [
        [
          'importer',
          {
            packageDependencies: [
              ['importer', 'importer'],
              ['haspeer', 'virtual:2.0.0_peer@1.0.0#2.0.0'],
              ['peer', '1.0.0'],
            ],
            packageLocation: 'importer',
          },
        ],
      ],
    ],
    [
      'haspeer',
      [
        [
          'virtual:2.0.0_peer@1.0.0#2.0.0',
          {
            packageDependencies: [
              ['haspeer', 'virtual:2.0.0_peer@1.0.0#2.0.0'],
              ['peer', '1.0.0'],
            ],
            packageLocation: '/home/zoli/.pnpm-store/2/haspeer@2.0.0_peer@1.0.0/node_modules/haspeer',
          },
        ],
      ],
    ],
    [
      'peer',
      [
        [
          '1.0.0',
          {
            packageDependencies: [
              ['peer', '1.0.0'],
            ],
            packageLocation: '/home/zoli/.pnpm-store/2/peer@1.0.0/node_modules/peer',
          },
        ],
      ],
    ],
  ])
})
