import { LOCKFILE_VERSION } from '@pnpm/constants'
import { filterLockfileByImportersAndEngine } from '@pnpm/filter-lockfile'

const REGIONAL_ARCH = Object.assign({}, process.arch)
const REGIONAL_CPU = Object.assign({}, process.platform)

jest.mock('detect-libc', () => {
  const orginal = jest.requireActual('detect-libc')
  return {
    ...orginal,
    familySync: () => 'musl',
  }
})

afterEach(() => {
  Object.defineProperties(process, {
    platform: {
      value: REGIONAL_CPU,
    },
    arch: {
      value: REGIONAL_ARCH,
    },
  })
})

test('filterByImportersAndEngine(): skip packages that are not installable', () => {
  const skippedPackages = new Set<string>(['/preserve-existing-skipped/1.0.0'])
  const filteredLockfile = filterLockfileByImportersAndEngine(
    {
      importers: {
        'project-1': {
          dependencies: {
            'prod-dep': '1.0.0',
          },
          devDependencies: {
            'dev-dep': '1.0.0',
          },
          optionalDependencies: {
            'not-skipped-optional': '1.0.0',
            'optional-dep': '1.0.0',
          },
          specifiers: {
            'dev-dep': '^1.0.0',
            'not-skipped-optional': '^1.0.0',
            'optional-dep': '^1.0.0',
            'prod-dep': '^1.0.0',
          },
        },
        'project-2': {
          dependencies: {
            'project-2-prod-dep': '1.0.0',
          },
          specifiers: {
            'project-2-prod-dep': '^1.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        '/bar/1.0.0': {
          resolution: { integrity: '' },
        },
        '/dev-dep/1.0.0': {
          dev: true,
          resolution: { integrity: '' },
        },
        '/foo/1.0.0': {
          optional: true,
          resolution: { integrity: '' },
        },
        '/not-skipped-optional/1.0.0': {
          optional: true,
          resolution: { integrity: '' },
        },
        '/optional-dep/1.0.0': {
          dependencies: {
            bar: '1.0.0',
            foo: '1.0.0',
          },
          engines: {
            node: '1000',
          },
          optional: true,
          resolution: { integrity: '' },
        },
        '/prod-dep-dep/1.0.0': {
          resolution: { integrity: '' },
        },
        '/prod-dep/1.0.0': {
          dependencies: {
            bar: '1.0.0',
            'prod-dep-dep': '1.0.0',
          },
          optionalDependencies: {
            'optional-dep': '1.0.0',
          },
          resolution: { integrity: '' },
        },
        '/project-2-prod-dep/1.0.0': {
          resolution: { integrity: '' },
        },
      },
    },
    ['project-1'],
    {
      currentEngine: {
        nodeVersion: '10.0.0',
        pnpmVersion: '2.0.0',
      },
      engineStrict: true,
      failOnMissingDependencies: true,
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: true,
      },
      lockfileDir: process.cwd(),
      skipped: skippedPackages,
    }
  )

  expect(filteredLockfile.lockfile).toStrictEqual({
    importers: {
      'project-1': {
        dependencies: {
          'prod-dep': '1.0.0',
        },
        devDependencies: {
          'dev-dep': '1.0.0',
        },
        optionalDependencies: {
          'not-skipped-optional': '1.0.0',
          'optional-dep': '1.0.0',
        },
        specifiers: {
          'dev-dep': '^1.0.0',
          'not-skipped-optional': '^1.0.0',
          'optional-dep': '^1.0.0',
          'prod-dep': '^1.0.0',
        },
      },
      'project-2': {
        dependencies: {
          'project-2-prod-dep': '1.0.0',
        },
        specifiers: {
          'project-2-prod-dep': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/bar/1.0.0': {
        resolution: { integrity: '' },
      },
      '/dev-dep/1.0.0': {
        dev: true,
        resolution: { integrity: '' },
      },
      '/foo/1.0.0': {
        optional: true,
        resolution: { integrity: '' },
      },
      '/not-skipped-optional/1.0.0': {
        optional: true,
        resolution: { integrity: '' },
      },
      '/optional-dep/1.0.0': {
        dependencies: {
          bar: '1.0.0',
          foo: '1.0.0',
        },
        engines: {
          node: '1000',
        },
        optional: true,
        resolution: { integrity: '' },
      },
      '/prod-dep-dep/1.0.0': {
        resolution: { integrity: '' },
      },
      '/prod-dep/1.0.0': {
        dependencies: {
          bar: '1.0.0',
          'prod-dep-dep': '1.0.0',
        },
        optionalDependencies: {
          'optional-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
    },
  })
  expect(Array.from(skippedPackages)).toStrictEqual(['/preserve-existing-skipped/1.0.0', '/optional-dep/1.0.0', '/foo/1.0.0'])
})

test('filterByImportersAndEngine(): filter the packages that set os and cpu', () => {
  Object.defineProperties(process, {
    platform: {
      value: 'darwin',
    },
    arch: {
      value: 'x64',
    },
  })

  const skippedPackages = new Set<string>(['/preserve-existing-skipped/1.0.0'])
  const filteredLockfile = filterLockfileByImportersAndEngine(
    {
      importers: {
        'project-1': {
          dependencies: {
            'prod-dep': '1.0.0',
          },
          devDependencies: {
            'dev-dep': '1.0.0',
          },
          optionalDependencies: {
            'not-skipped-optional': '1.0.0',
            'optional-dep': '1.0.0',
          },
          specifiers: {
            'dev-dep': '^1.0.0',
            'not-skipped-optional': '^1.0.0',
            'optional-dep': '^1.0.0',
            'prod-dep': '^1.0.0',
          },
        },
        'project-2': {
          dependencies: {
            'project-2-prod-dep': '1.0.0',
          },
          specifiers: {
            'project-2-prod-dep': '^1.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        '/bar/1.0.0': {
          resolution: { integrity: '' },
        },
        '/dev-dep/1.0.0': {
          dev: true,
          resolution: { integrity: '' },
        },
        '/foo/1.0.0': {
          optional: true,
          resolution: { integrity: '' },
        },
        '/not-skipped-optional/1.0.0': {
          optional: true,
          resolution: { integrity: '' },
        },
        '/optional-dep/1.0.0': {
          dependencies: {
            bar: '1.0.0',
            foo: '1.0.0',
          },
          os: ['linux'],
          cpu: ['x64'],
          optional: true,
          resolution: { integrity: '' },
        },
        '/prod-dep-dep/1.0.0': {
          resolution: { integrity: '' },
        },
        '/prod-dep/1.0.0': {
          dependencies: {
            bar: '1.0.0',
            'prod-dep-dep': '1.0.0',
          },
          optionalDependencies: {
            'optional-dep': '1.0.0',
          },
          resolution: { integrity: '' },
        },
        '/project-2-prod-dep/1.0.0': {
          resolution: { integrity: '' },
        },
      },
    },
    ['project-1'],
    {
      currentEngine: {
        nodeVersion: '10.0.0',
        pnpmVersion: '2.0.0',
      },
      engineStrict: true,
      failOnMissingDependencies: true,
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: true,
      },
      lockfileDir: process.cwd(),
      skipped: skippedPackages,
    }
  )

  expect(filteredLockfile.lockfile).toStrictEqual({
    importers: {
      'project-1': {
        dependencies: {
          'prod-dep': '1.0.0',
        },
        devDependencies: {
          'dev-dep': '1.0.0',
        },
        optionalDependencies: {
          'not-skipped-optional': '1.0.0',
          'optional-dep': '1.0.0',
        },
        specifiers: {
          'dev-dep': '^1.0.0',
          'not-skipped-optional': '^1.0.0',
          'optional-dep': '^1.0.0',
          'prod-dep': '^1.0.0',
        },
      },
      'project-2': {
        dependencies: {
          'project-2-prod-dep': '1.0.0',
        },
        specifiers: {
          'project-2-prod-dep': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/bar/1.0.0': {
        resolution: { integrity: '' },
      },
      '/dev-dep/1.0.0': {
        dev: true,
        resolution: { integrity: '' },
      },
      '/foo/1.0.0': {
        optional: true,
        resolution: { integrity: '' },
      },
      '/not-skipped-optional/1.0.0': {
        optional: true,
        resolution: { integrity: '' },
      },
      '/optional-dep/1.0.0': {
        dependencies: {
          bar: '1.0.0',
          foo: '1.0.0',
        },
        os: ['linux'],
        cpu: ['x64'],
        optional: true,
        resolution: { integrity: '' },
      },
      '/prod-dep-dep/1.0.0': {
        resolution: { integrity: '' },
      },
      '/prod-dep/1.0.0': {
        dependencies: {
          bar: '1.0.0',
          'prod-dep-dep': '1.0.0',
        },
        optionalDependencies: {
          'optional-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
    },
  })
  expect(Array.from(skippedPackages)).toStrictEqual(['/preserve-existing-skipped/1.0.0', '/optional-dep/1.0.0', '/foo/1.0.0'])
})

test('filterByImportersAndEngine(): filter the packages that set libc', () => {
  const skippedPackages = new Set<string>(['/preserve-existing-skipped/1.0.0'])
  const filteredLockfile = filterLockfileByImportersAndEngine(
    {
      importers: {
        'project-1': {
          dependencies: {
            'prod-dep': '1.0.0',
          },
          devDependencies: {
            'dev-dep': '1.0.0',
          },
          optionalDependencies: {
            'not-skipped-optional': '1.0.0',
            'optional-dep': '1.0.0',
          },
          specifiers: {
            'dev-dep': '^1.0.0',
            'not-skipped-optional': '^1.0.0',
            'optional-dep': '^1.0.0',
            'prod-dep': '^1.0.0',
          },
        },
        'project-2': {
          dependencies: {
            'project-2-prod-dep': '1.0.0',
          },
          specifiers: {
            'project-2-prod-dep': '^1.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        '/bar/1.0.0': {
          resolution: { integrity: '' },
        },
        '/dev-dep/1.0.0': {
          dev: true,
          resolution: { integrity: '' },
        },
        '/foo/1.0.0': {
          optional: true,
          resolution: { integrity: '' },
        },
        '/not-skipped-optional/1.0.0': {
          optional: true,
          resolution: { integrity: '' },
        },
        '/optional-dep/1.0.0': {
          dependencies: {
            bar: '1.0.0',
            foo: '1.0.0',
          },
          libc: ['glibc'],
          optional: true,
          resolution: { integrity: '' },
        },
        '/prod-dep-dep/1.0.0': {
          resolution: { integrity: '' },
        },
        '/prod-dep/1.0.0': {
          dependencies: {
            bar: '1.0.0',
            'prod-dep-dep': '1.0.0',
          },
          optionalDependencies: {
            'optional-dep': '1.0.0',
          },
          resolution: { integrity: '' },
        },
        '/project-2-prod-dep/1.0.0': {
          resolution: { integrity: '' },
        },
      },
    },
    ['project-1'],
    {
      currentEngine: {
        nodeVersion: '10.0.0',
        pnpmVersion: '2.0.0',
      },
      engineStrict: true,
      failOnMissingDependencies: true,
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: true,
      },
      lockfileDir: process.cwd(),
      skipped: skippedPackages,
    }
  )

  expect(filteredLockfile.lockfile).toStrictEqual({
    importers: {
      'project-1': {
        dependencies: {
          'prod-dep': '1.0.0',
        },
        devDependencies: {
          'dev-dep': '1.0.0',
        },
        optionalDependencies: {
          'not-skipped-optional': '1.0.0',
          'optional-dep': '1.0.0',
        },
        specifiers: {
          'dev-dep': '^1.0.0',
          'not-skipped-optional': '^1.0.0',
          'optional-dep': '^1.0.0',
          'prod-dep': '^1.0.0',
        },
      },
      'project-2': {
        dependencies: {
          'project-2-prod-dep': '1.0.0',
        },
        specifiers: {
          'project-2-prod-dep': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/bar/1.0.0': {
        resolution: { integrity: '' },
      },
      '/dev-dep/1.0.0': {
        dev: true,
        resolution: { integrity: '' },
      },
      '/foo/1.0.0': {
        optional: true,
        resolution: { integrity: '' },
      },
      '/not-skipped-optional/1.0.0': {
        optional: true,
        resolution: { integrity: '' },
      },
      '/optional-dep/1.0.0': {
        dependencies: {
          bar: '1.0.0',
          foo: '1.0.0',
        },
        libc: ['glibc'],
        optional: true,
        resolution: { integrity: '' },
      },
      '/prod-dep-dep/1.0.0': {
        resolution: { integrity: '' },
      },
      '/prod-dep/1.0.0': {
        dependencies: {
          bar: '1.0.0',
          'prod-dep-dep': '1.0.0',
        },
        optionalDependencies: {
          'optional-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
    },
  })
  expect(Array.from(skippedPackages)).toStrictEqual(['/preserve-existing-skipped/1.0.0', '/optional-dep/1.0.0', '/foo/1.0.0'])
})

test('filterByImportersAndEngine(): includes linked packages', () => {
  const filteredLockfile = filterLockfileByImportersAndEngine(
    {
      importers: {
        'project-1': {
          dependencies: {
            'project-2': 'link:project-2',
          },
          devDependencies: {
          },
          optionalDependencies: {
          },
          specifiers: {
            'project-2': '^1.0.0',
          },
        },
        'project-2': {
          dependencies: {
            'project-3': 'link:project-3',
            foo: '1.0.0',
          },
          specifiers: {
            foo: '^1.0.0',
          },
        },
        'project-3': {
          dependencies: {
            bar: '1.0.0',
          },
          specifiers: {
            bar: '^1.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        '/bar/1.0.0': {
          resolution: { integrity: '' },
        },
        '/foo/1.0.0': {
          resolution: { integrity: '' },
        },
      },
    },
    ['project-1'],
    {
      currentEngine: {
        nodeVersion: '10.0.0',
        pnpmVersion: '2.0.0',
      },
      engineStrict: true,
      failOnMissingDependencies: true,
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: true,
      },
      lockfileDir: process.cwd(),
      skipped: new Set(),
    }
  )

  expect(filteredLockfile.lockfile).toStrictEqual({
    importers: {
      'project-1': {
        dependencies: {
          'project-2': 'link:project-2',
        },
        devDependencies: {
        },
        optionalDependencies: {
        },
        specifiers: {
          'project-2': '^1.0.0',
        },
      },
      'project-2': {
        dependencies: {
          'project-3': 'link:project-3',
          foo: '1.0.0',
        },
        specifiers: {
          foo: '^1.0.0',
        },
      },
      'project-3': {
        dependencies: {
          bar: '1.0.0',
        },
        specifiers: {
          bar: '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/bar/1.0.0': {
        resolution: { integrity: '' },
      },
      '/foo/1.0.0': {
        resolution: { integrity: '' },
      },
    },
  })
  expect(filteredLockfile.selectedImporterIds).toStrictEqual([
    'project-1',
    'project-2',
    'project-3',
  ])
})
