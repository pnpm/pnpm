import { WANTED_SHRINKWRAP_FILENAME } from '@pnpm/constants'
import { filterByImporters } from '@pnpm/filter-shrinkwrap'
import test = require('tape')

test('filterByImporters(): only prod dependencies of one importer', (t) => {
  const filteredShr = filterByImporters(
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
          }
        }
      },
      lockfileVersion: 5,
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
      registries: {
        default: 'https://registry.npmjs.org/',
      },
      skipped: new Set<string>(),
    },
  )

  t.deepEqual(filteredShr, {
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
        }
      }
    },
    lockfileVersion: 5,
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
  t.end()
})

// TODO: also fail when filterShrinkwrap() is used
test('filterByImporters(): fail on missing packages when failOnMissingDependencies is true', (t) => {
  let err!: Error
  try {
    filterByImporters(
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
        lockfileVersion: 5,
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
        registries: {
          default: 'https://registry.npmjs.org/',
        },
        skipped: new Set<string>(),
      },
    )
  } catch (_) {
    err = _
  }
  t.ok(err)
  t.equal(err.message, `No entry for "/prod-dep-dep/1.0.0" in ${WANTED_SHRINKWRAP_FILENAME}`)
  t.end()
})

test('filterByImporters(): do not fail on missing packages when failOnMissingDependencies is false', (t) => {
  const filteredShr = filterByImporters(
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
      lockfileVersion: 5,
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
      registries: {
        default: 'https://registry.npmjs.org/',
      },
      skipped: new Set<string>(),
    },
  )

  t.deepEqual(filteredShr, {
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
    lockfileVersion: 5,
    packages: {
      '/prod-dep/1.0.0': {
        dependencies: {
          'prod-dep-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
    },
  })

  t.end()
})

test('filterByImporters(): do not include skipped packages', (t) => {
  const filteredShr = filterByImporters(
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
          }
        }
      },
      lockfileVersion: 5,
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
      registries: {
        default: 'https://registry.npmjs.org/',
      },
      skipped: new Set<string>(['/optional-dep/1.0.0']),
    },
  )

  t.deepEqual(filteredShr, {
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
        }
      }
    },
    lockfileVersion: 5,
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
  t.end()
})
