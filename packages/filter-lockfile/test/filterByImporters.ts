import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { filterLockfileByImporters } from '@pnpm/filter-lockfile'

test('filterByImporters(): only prod dependencies of one importer', () => {
  const filteredLockfile = filterLockfileByImporters(
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
            'optional-dep': '1.0.0',
          },
          specifiers: {
            'dev-dep': '^1.0.0',
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
        '/dev-dep/1.0.0': {
          dev: true,
          resolution: { integrity: '' },
        },
        '/optional-dep/1.0.0': {
          optional: true,
          resolution: { integrity: '' },
        },
        '/prod-dep-dep/1.0.0': {
          resolution: { integrity: '' },
        },
        '/prod-dep/1.0.0': {
          dependencies: {
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
      failOnMissingDependencies: true,
      include: {
        dependencies: true,
        devDependencies: false,
        optionalDependencies: false,
      },
      skipped: new Set<string>(),
    }
  )

  expect(filteredLockfile).toStrictEqual({
    importers: {
      'project-1': {
        dependencies: {
          'prod-dep': '1.0.0',
        },
        devDependencies: {},
        optionalDependencies: {},
        specifiers: {
          'dev-dep': '^1.0.0',
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
      '/prod-dep-dep/1.0.0': {
        resolution: { integrity: '' },
      },
      '/prod-dep/1.0.0': {
        dependencies: {
          'prod-dep-dep': '1.0.0',
        },
        optionalDependencies: {
          'optional-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
    },
  })
})

// TODO: also fail when filterLockfile() is used
test('filterByImporters(): fail on missing packages when failOnMissingDependencies is true', () => {
  let err!: Error
  try {
    filterLockfileByImporters(
      {
        importers: {
          'project-1': {
            dependencies: {
              'prod-dep': '1.0.0',
            },
            specifiers: {
              'prod-dep': '^1.0.0',
            },
          },
          'project-2': {
            specifiers: {},
          },
        },
        lockfileVersion: LOCKFILE_VERSION,
        packages: {
          '/prod-dep/1.0.0': {
            dependencies: {
              'prod-dep-dep': '1.0.0',
            },
            resolution: {
              integrity: '',
            },
          },
        },
      },
      ['project-1'],
      {
        failOnMissingDependencies: true,
        include: {
          dependencies: true,
          devDependencies: false,
          optionalDependencies: false,
        },
        skipped: new Set<string>(),
      }
    )
  } catch (_) {
    err = _
  }
  expect(err).not.toBeNull()
  expect(err.message).toEqual(`Broken lockfile: no entry for '/prod-dep-dep/1.0.0' in ${WANTED_LOCKFILE}`)
})

test('filterByImporters(): do not fail on missing packages when failOnMissingDependencies is false', () => {
  const filteredLockfile = filterLockfileByImporters(
    {
      importers: {
        'project-1': {
          dependencies: {
            'prod-dep': '1.0.0',
          },
          specifiers: {
            'prod-dep': '^1.0.0',
          },
        },
        'project-2': {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        '/prod-dep/1.0.0': {
          dependencies: {
            'prod-dep-dep': '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
      },
    },
    ['project-1'],
    {
      failOnMissingDependencies: false,
      include: {
        dependencies: true,
        devDependencies: false,
        optionalDependencies: false,
      },
      skipped: new Set<string>(),
    }
  )

  expect(filteredLockfile).toStrictEqual({
    importers: {
      'project-1': {
        dependencies: {
          'prod-dep': '1.0.0',
        },
        devDependencies: {},
        optionalDependencies: {},
        specifiers: {
          'prod-dep': '^1.0.0',
        },
      },
      'project-2': {
        specifiers: {},
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/prod-dep/1.0.0': {
        dependencies: {
          'prod-dep-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
    },
  })
})

test('filterByImporters(): do not include skipped packages', () => {
  const filteredLockfile = filterLockfileByImporters(
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
            'optional-dep': '1.0.0',
          },
          specifiers: {
            'dev-dep': '^1.0.0',
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
        '/dev-dep/1.0.0': {
          dev: true,
          resolution: { integrity: '' },
        },
        '/optional-dep/1.0.0': {
          optional: true,
          resolution: { integrity: '' },
        },
        '/prod-dep-dep/1.0.0': {
          resolution: { integrity: '' },
        },
        '/prod-dep/1.0.0': {
          dependencies: {
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
      failOnMissingDependencies: true,
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: true,
      },
      skipped: new Set<string>(['/optional-dep/1.0.0']),
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
          'optional-dep': '1.0.0',
        },
        specifiers: {
          'dev-dep': '^1.0.0',
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
      '/dev-dep/1.0.0': {
        dev: true,
        resolution: { integrity: '' },
      },
      '/prod-dep-dep/1.0.0': {
        resolution: { integrity: '' },
      },
      '/prod-dep/1.0.0': {
        dependencies: {
          'prod-dep-dep': '1.0.0',
        },
        optionalDependencies: {
          'optional-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
    },
  })
})

test('filterByImporters(): exclude orphan packages', () => {
  const filteredLockfile = filterLockfileByImporters(
    {
      importers: {
        'project-1': {
          dependencies: {
            'prod-dep': '1.0.0',
          },
          specifiers: {
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
        '/orphan/1.0.0': {
          resolution: { integrity: '' },
        },
        '/prod-dep-dep/1.0.0': {
          resolution: { integrity: '' },
        },
        '/prod-dep/1.0.0': {
          dependencies: {
            'prod-dep-dep': '1.0.0',
          },
          resolution: { integrity: '' },
        },
        '/project-2-prod-dep/1.0.0': {
          resolution: { integrity: '' },
        },
      },
    },
    ['project-1', 'project-2'],
    {
      failOnMissingDependencies: true,
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: true,
      },
      skipped: new Set<string>(),
    }
  )

  expect(filteredLockfile).toStrictEqual({
    importers: {
      'project-1': {
        dependencies: {
          'prod-dep': '1.0.0',
        },
        devDependencies: {},
        optionalDependencies: {},
        specifiers: {
          'prod-dep': '^1.0.0',
        },
      },
      'project-2': {
        dependencies: {
          'project-2-prod-dep': '1.0.0',
        },
        devDependencies: {},
        optionalDependencies: {},
        specifiers: {
          'project-2-prod-dep': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/prod-dep-dep/1.0.0': {
        resolution: { integrity: '' },
      },
      '/prod-dep/1.0.0': {
        dependencies: {
          'prod-dep-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
      '/project-2-prod-dep/1.0.0': {
        resolution: { integrity: '' },
      },
    },
  })
})
