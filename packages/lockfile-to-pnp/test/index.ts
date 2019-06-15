import { lockfileToPackageRegistry } from '@pnpm/lockfile-to-pnp'
import test = require('tape')

test('lockfileToPackageRegistry', (t) => {
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
    storeDirectory: '/home/zoli/.pnpm-store/2',
  })

  const actual = Array
  .from(packageRegistry.entries())
  .map(([packageName, packageStoreMap]) => {
    return [
      packageName,
      Array.from(packageStoreMap.entries())
        .map(([pkgRef, packageInfo]) => {
          return [
            pkgRef,
            { packageLocation: packageInfo.packageLocation, packageDependencies: Array.from(packageInfo.packageDependencies || new Map()) },
          ]
        })
    ]
  })
  t.deepEqual(actual, [
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
            packageLocation: '/home/zoli/.pnpm-store/2/registry.npmjs.org/dep1/1.0.0/node_modules/dep1',
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
            packageLocation: '/home/zoli/.pnpm-store/2/registry.npmjs.org/foo/2.0.0/node_modules/foo',
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
            packageLocation: '/home/zoli/.pnpm-store/2/registry.npmjs.org/qar/2.0.0/node_modules/qar',
          },
        ],
        [
          '3.0.0',
          {
            packageDependencies: [
              ['qar', '3.0.0'],
            ],
            packageLocation: '/home/zoli/.pnpm-store/2/registry.npmjs.org/qar/3.0.0/node_modules/qar',
          }
        ]
      ]
    ]
  ])

  t.end()
})
