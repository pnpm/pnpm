// cspell:ignore haspeer
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { dependenciesGraphToPackageMap, lockfileToPackageMap, lockfileToPackageRegistry } from '@pnpm/lockfile.to-pnp'
import type { DepPath, ProjectId } from '@pnpm/types'

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

test('lockfileToPackageMap', () => {
  const packageMap = lockfileToPackageMap({
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          dep1: '1.0.0',
          dep2Alias: 'foo@2.0.0',
          linked: 'link:packages/linked',
        },
        specifiers: {},
      },
      ['packages/app' as ProjectId]: {
        dependencies: {
          dep1: '1.0.0',
          linked: 'link:../linked',
        },
        devDependencies: {
          dep2Alias: 'foo@2.0.0',
        },
        specifiers: {},
      },
      ['packages/linked' as ProjectId]: {
        dependencies: {
          qar: '3.0.0',
        },
        specifiers: {},
      },
    },
    lockfileVersion: '5',
    packages: {
      ['dep1@1.0.0' as DepPath]: {
        dependencies: {
          dep2Alias: 'foo@2.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      ['foo@2.0.0' as DepPath]: {
        optionalDependencies: {
          qar: '3.0.0',
        },
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
      '.': 'root',
      'packages/app': 'app',
      'packages/linked': 'linked',
    },
    lockfileDir: process.cwd(),
    rootModulesDir: path.resolve('node_modules'),
    virtualStoreDir: path.resolve('node_modules/.pnpm'),
    virtualStoreDirMaxLength: 120,
  })

  expect(packageMap).toStrictEqual({
    packages: {
      '.': {
        url: '..',
        dependencies: {
          dep1: 'dep1@1.0.0',
          dep2Alias: 'foo@2.0.0',
          linked: 'packages/linked',
          root: '.',
        },
      },
      'dep1@1.0.0': {
        url: './.pnpm/dep1@1.0.0/node_modules/dep1',
        dependencies: {
          dep1: 'dep1@1.0.0',
          dep2Alias: 'foo@2.0.0',
        },
      },
      'foo@2.0.0': {
        url: './.pnpm/foo@2.0.0/node_modules/foo',
        dependencies: {
          foo: 'foo@2.0.0',
          qar: 'qar@3.0.0',
        },
      },
      'packages/app': {
        url: '../packages/app',
        dependencies: {
          app: 'packages/app',
          dep1: 'dep1@1.0.0',
          dep2Alias: 'foo@2.0.0',
          linked: 'packages/linked',
        },
      },
      'packages/linked': {
        url: '../packages/linked',
        dependencies: {
          linked: 'packages/linked',
          qar: 'qar@3.0.0',
        },
      },
      'qar@3.0.0': {
        url: './.pnpm/qar@3.0.0/node_modules/qar',
        dependencies: {
          qar: 'qar@3.0.0',
        },
      },
    },
  })
})

test('lockfileToPackageMap loose mode includes linked dependencies from physical ancestors', () => {
  const lockfile = {
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          dep1: '1.0.0',
          linked: 'link:packages/linked',
        },
        specifiers: {},
      },
    },
    lockfileVersion: '5',
    packages: {
      ['dep1@1.0.0' as DepPath]: {
        resolution: {
          integrity: '',
        },
      },
    },
  }
  const opts = {
    importerNames: {
      '.': 'root',
    },
    lockfileDir: process.cwd(),
    rootModulesDir: path.resolve('node_modules'),
    virtualStoreDir: path.resolve('node_modules/.pnpm'),
    virtualStoreDirMaxLength: 120,
  }
  const standardPackageMap = lockfileToPackageMap(lockfile, opts)
  const loosePackageMap = lockfileToPackageMap(lockfile, {
    ...opts,
    packageMapType: 'loose',
  })

  expect(standardPackageMap.packages['dep1@1.0.0'].dependencies).toStrictEqual({
    dep1: 'dep1@1.0.0',
  })

  expect(loosePackageMap.packages['dep1@1.0.0'].dependencies).toStrictEqual({
    dep1: 'dep1@1.0.0',
    linked: 'packages/linked',
  })
})

test('lockfileToPackageMap uses file urls and link ids for Windows cross-drive links', () => {
  const packageMap = lockfileToPackageMap({
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          linked: 'link:D:\\external\\linked',
        },
        specifiers: {},
      },
    },
    lockfileVersion: '5',
    packages: {},
  }, {
    importerNames: {},
    lockfileDir: 'C:\\repo',
    rootModulesDir: 'C:\\repo\\node_modules',
    virtualStoreDir: 'C:\\repo\\node_modules\\.pnpm',
    virtualStoreDirMaxLength: 120,
  })

  expect(packageMap.packages['.'].dependencies).toStrictEqual({
    linked: 'link:D:/external/linked',
  })
  expect(packageMap.packages['link:D:/external/linked'].url).toBe('file:///D:/external/linked')
})

test('lockfileToPackageMap detects a Windows-absolute link target on a POSIX lockfile dir', () => {
  const packageMap = lockfileToPackageMap({
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          linked: 'link:C:\\external\\linked',
        },
        specifiers: {},
      },
    },
    lockfileVersion: '5',
    packages: {},
  }, {
    importerNames: {},
    lockfileDir: '/repo',
    rootModulesDir: '/repo/node_modules',
    virtualStoreDir: '/repo/node_modules/.pnpm',
    virtualStoreDirMaxLength: 120,
  })

  // Without stripping `link:` before picking the path flavor, the target is
  // misread as a relative path and resolved under the importer dir.
  expect(packageMap.packages['.'].dependencies).toStrictEqual({
    linked: 'link:C:/external/linked',
  })
  expect(packageMap.packages['link:C:/external/linked'].url).toBe('file:///C:/external/linked')
})

test('dependenciesGraphToPackageMap uses file urls and link ids for Windows cross-drive links', () => {
  const packageMap = dependenciesGraphToPackageMap({
    directDependenciesByImporterId: {
      '.': {},
    },
    graph: {},
    importerNames: {
      '.': 'root',
    },
    lockfile: {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            linked: 'link:D:\\external\\linked',
          },
          specifiers: {},
        },
      },
      lockfileVersion: '5',
      packages: {},
    },
    lockfileDir: 'C:\\repo',
    packageIdStrategy: 'path',
    rootModulesDir: 'C:\\repo\\node_modules',
  })

  expect(packageMap.packages['.'].dependencies).toStrictEqual({
    linked: 'link:D:/external/linked',
    root: '.',
  })
  expect(packageMap.packages['link:D:/external/linked'].url).toBe('file:///D:/external/linked')
})

test('dependenciesGraphToPackageMap loose mode includes linked dependencies from physical ancestors', () => {
  const rootModulesDir = path.resolve('node_modules')
  const dep1Dir = path.join(rootModulesDir, 'dep1')
  const packageMap = dependenciesGraphToPackageMap({
    directDependenciesByImporterId: {
      '.': {
        dep1: dep1Dir,
      },
    },
    graph: {
      [dep1Dir]: {
        children: {},
        depPath: 'dep1@1.0.0' as DepPath,
        dir: dep1Dir,
        name: 'dep1',
      },
    },
    importerNames: {
      '.': 'root',
    },
    lockfile: {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            dep1: '1.0.0',
            linked: 'link:packages/linked',
          },
          specifiers: {},
        },
      },
      lockfileVersion: '5',
      packages: {
        ['dep1@1.0.0' as DepPath]: {
          resolution: {
            integrity: '',
          },
        },
      },
    },
    lockfileDir: process.cwd(),
    packageIdStrategy: 'path',
    packageMapType: 'loose',
    rootModulesDir,
  })

  expect(packageMap.packages.dep1.dependencies).toStrictEqual({
    dep1: 'dep1',
    linked: '../packages/linked',
  })
})

test('dependenciesGraphToPackageMap with path package ids', () => {
  const packageMap = dependenciesGraphToPackageMap({
    directDependenciesByImporterId: {
      '.': {
        dep1: path.resolve('node_modules/dep1'),
      },
      'packages/app': {
        dep1: path.resolve('packages/app/node_modules/dep1'),
      },
    },
    graph: {
      [path.resolve('node_modules/dep1')]: {
        children: {
          dep2Alias: path.resolve('node_modules/foo'),
        },
        depPath: 'dep1@1.0.0' as DepPath,
        dir: path.resolve('node_modules/dep1'),
        name: 'dep1',
      },
      [path.resolve('node_modules/foo')]: {
        children: {},
        depPath: 'foo@2.0.0' as DepPath,
        dir: path.resolve('node_modules/foo'),
        name: 'foo',
      },
      [path.resolve('packages/app/node_modules/dep1')]: {
        children: {
          dep2Alias: path.resolve('node_modules/foo'),
        },
        depPath: 'dep1@1.0.0' as DepPath,
        dir: path.resolve('packages/app/node_modules/dep1'),
        name: 'dep1',
      },
    },
    importerNames: {
      '.': 'root',
      'packages/app': 'app',
    },
    lockfile: {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            dep1: '1.0.0',
          },
          specifiers: {},
        },
        ['packages/app' as ProjectId]: {
          dependencies: {
            dep1: '1.0.0',
          },
          specifiers: {},
        },
      },
      lockfileVersion: '5',
      packages: {
        ['dep1@1.0.0' as DepPath]: {
          dependencies: {
            dep2Alias: 'foo@2.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        ['foo@2.0.0' as DepPath]: {
          resolution: {
            integrity: '',
          },
        },
      },
    },
    lockfileDir: process.cwd(),
    packageIdStrategy: 'path',
    rootModulesDir: path.resolve('node_modules'),
  })

  expect(packageMap).toStrictEqual({
    packages: {
      '.': {
        url: '..',
        dependencies: {
          dep1: 'dep1',
          root: '.',
        },
      },
      dep1: {
        url: './dep1',
        dependencies: {
          dep1: 'dep1',
          dep2Alias: 'foo',
        },
      },
      foo: {
        url: './foo',
        dependencies: {
          foo: 'foo',
        },
      },
      '../packages/app': {
        url: '../packages/app',
        dependencies: {
          app: '../packages/app',
          dep1: '../packages/app/node_modules/dep1',
        },
      },
      '../packages/app/node_modules/dep1': {
        url: '../packages/app/node_modules/dep1',
        dependencies: {
          dep1: '../packages/app/node_modules/dep1',
          dep2Alias: 'foo',
        },
      },
    },
  })
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
