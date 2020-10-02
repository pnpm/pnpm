import { lockfileToPackageRegistry } from '../lib'
import path = require('path')

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
    lockfileDir: process.cwd(),
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    virtualStoreDir: path.resolve('node_modules/.pnpm'),
  })

  const actual = Array.from(
    packageRegistry,
    ([packageName, packageStoreMap]) => {
      return [
        packageName,
        Array.from(
          packageStoreMap,
          ([pkgRef, packageInfo]) => {
            return [
              pkgRef,
              {
                packageDependencies: Array.from(packageInfo.packageDependencies),
                packageLocation: packageInfo.packageLocation,
              },
            ]
          }
        ),
      ]
    }
  )

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
            packageLocation: './importer1',
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
            packageLocation: './importer2',
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
            packageLocation: './node_modules/.pnpm/dep1@1.0.0/node_modules/dep1',
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
            packageLocation: './node_modules/.pnpm/foo@2.0.0/node_modules/foo',
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
            packageLocation: './node_modules/.pnpm/qar@2.0.0/node_modules/qar',
          },
        ],
        [
          '3.0.0',
          {
            packageDependencies: [
              ['qar', '3.0.0'],
            ],
            packageLocation: './node_modules/.pnpm/qar@3.0.0/node_modules/qar',
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
    lockfileDir: process.cwd(),
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    virtualStoreDir: path.resolve('node_modules/.pnpm'),
  })

  const actual = Array.from(
    packageRegistry,
    ([packageName, packageStoreMap]) => {
      return [
        packageName,
        Array.from(
          packageStoreMap,
          ([pkgRef, packageInfo]) => {
            return [
              pkgRef,
              {
                packageDependencies: Array.from(packageInfo.packageDependencies),
                packageLocation: packageInfo.packageLocation,
              },
            ]
          }
        ),
      ]
    }
  )

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
            packageLocation: './importer',
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
            packageLocation: './node_modules/.pnpm/haspeer@2.0.0_peer@1.0.0/node_modules/haspeer',
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
            packageLocation: './node_modules/.pnpm/peer@1.0.0/node_modules/peer',
          },
        ],
      ],
    ],
  ])
})
