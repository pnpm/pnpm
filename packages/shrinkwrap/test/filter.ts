import test = require('tape')
import { filterByImporters } from 'pnpm-shrinkwrap'

test('filter: only prod dependencies of one importer', (t) => {
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
      packages: {
        '/dev-dep/1.0.0': {
          dev: true,
          resolution: { integrity: '' },
        },
        '/optional-dep/1.0.0': {
          optional: true,
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
        '/prod-dep-dep/1.0.0': {
          resolution: { integrity: '' },
        },
        '/project-2-prod-dep/1.0.0': {
          resolution: { integrity: '' },
        },
      },
      registry: 'https://registry.npmjs.org/',
      shrinkwrapVersion: 3,
    },
    ['project-1'],
    {
      include: {
        dependencies: true,
        devDependencies: false,
        optionalDependencies: false,
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
    packages: {
      '/prod-dep/1.0.0': {
        dependencies: {
          'prod-dep-dep': '1.0.0',
        },
        optionalDependencies: {
          'optional-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
      '/prod-dep-dep/1.0.0': {
        resolution: { integrity: '' },
      },
    },
    registry: 'https://registry.npmjs.org/',
    shrinkwrapVersion: 3,
  })
  t.end()
})
