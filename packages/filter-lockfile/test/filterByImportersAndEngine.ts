import { LOCKFILE_VERSION } from '@pnpm/constants'
import { filterLockfileByImportersAndEngine } from '@pnpm/filter-lockfile'

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

  expect(filteredLockfile).toStrictEqual({
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
