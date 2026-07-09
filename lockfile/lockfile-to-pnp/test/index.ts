// cspell:ignore haspeer
import path from 'path'
import { lockfileToPackageRegistry } from '@pnpm/lockfile-to-pnp'
import { type DepPath, type ProjectId } from '@pnpm/types'

test('lockfileToPackageRegistry', () => {
  const packageRegistry = lockfileToPackageRegistry({
    importers: {
      ['importer1' as ProjectId]: {
        dependencies: {
          dep1: '1.0.0',
          dep2: 'foo@2.0.0',
        },
        optionalDependencies: {
          qar: '2.0.0',
        },
        specifiers: {},
      },
      ['importer2' as ProjectId]: {
        devDependencies: {
          importer1: 'link:../importer1',
        },
        specifiers: {},
      },
    },
    lockfileVersion: '5',
    packages: {
      ['dep1@1.0.0' as DepPath]: {
        dependencies: {
          dep2: 'foo@2.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      ['foo@2.0.0' as DepPath]: {
        dependencies: {
          qar: '3.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      ['qar@2.0.0' as DepPath]: {
        resolution: {
          integrity: '',
        },
      },
      ['qar@3.0.0' as DepPath]: {
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
    virtualStoreDirMaxLength: 120,
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
            packageLocation: './node_modules/.pnpm/dep1@1.0.0/node_modules/dep1/',
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
            packageLocation: './node_modules/.pnpm/foo@2.0.0/node_modules/foo/',
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
            packageLocation: './node_modules/.pnpm/qar@2.0.0/node_modules/qar/',
          },
        ],
        [
          '3.0.0',
          {
            packageDependencies: [
              ['qar', '3.0.0'],
            ],
            packageLocation: './node_modules/.pnpm/qar@3.0.0/node_modules/qar/',
          },
        ],
      ],
    ],
  ])
})

test('lockfileToPackageRegistry packages that have peer deps', () => {
  const packageRegistry = lockfileToPackageRegistry({
    importers: {
      ['importer' as ProjectId]: {
        dependencies: {
          haspeer: '2.0.0(peer@1.0.0)',
          peer: '1.0.0',
        },
        specifiers: {},
      },
    },
    lockfileVersion: '5',
    packages: {
      ['haspeer@2.0.0(peer@1.0.0)' as DepPath]: {
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
      ['peer@1.0.0' as DepPath]: {
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
    virtualStoreDirMaxLength: 120,
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
              ['haspeer', 'virtual:2.0.0(peer@1.0.0)#2.0.0'],
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
          'virtual:2.0.0(peer@1.0.0)#2.0.0',
          {
            packageDependencies: [
              ['haspeer', 'virtual:2.0.0(peer@1.0.0)#2.0.0'],
              ['peer', '1.0.0'],
            ],
            packageLocation: './node_modules/.pnpm/haspeer@2.0.0_peer@1.0.0/node_modules/haspeer/',
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
            packageLocation: './node_modules/.pnpm/peer@1.0.0/node_modules/peer/',
          },
        ],
      ],
    ],
  ])
})

// A `packages` depPath *key* whose name portion is a path-traversal makes the
// PnP `packageLocation` (built by joining that name onto the virtual store)
// point outside the store. The name must be rejected so a tampered lockfile
// can't aim the `.pnp.cjs` resolver map at arbitrary paths (GHSA-c59q-g84q-2gj5).
test('lockfileToPackageRegistry rejects a package name with path-traversal', () => {
  const lockfile = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        dependencies: { 'legit-name': '../../../escape@1.0.0' },
        specifiers: { 'legit-name': '1.0.0' },
      },
    },
    packages: {
      ['../../../escape@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
      },
    },
  } as unknown as Parameters<typeof lockfileToPackageRegistry>[0]
  expect(() =>
    lockfileToPackageRegistry(lockfile, {
      importerNames: {},
      lockfileDir: '/home/user/project',
      virtualStoreDir: '/home/user/project/node_modules/.pnpm',
      virtualStoreDirMaxLength: 120,
      registries: {
        default: 'https://registry.npmjs.org/',
      },
    })
  ).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})
