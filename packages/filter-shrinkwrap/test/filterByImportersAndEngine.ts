import { filterByImportersAndEngine } from '@pnpm/filter-shrinkwrap'
import test = require('tape')

test('filterByImportersAndEngine(): skip packages that are not installable', (t) => {
  const skippedPackages = new Set<string>(['registry.npmjs.org/preserve-existing-skipped/1.0.0'])
  const filteredShrinkwrap = filterByImportersAndEngine(
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
          }
        }
      },
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
            'bar': '1.0.0',
            'foo': '1.0.0',
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
            'bar': '1.0.0',
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
      shrinkwrapVersion: 4,
    },
    ['project-1'],
    {
      currentEngine: {
        nodeVersion: '10.0.0',
        pnpmVersion: '2.0.0',
      },
      defaultRegistry: 'https://registry.npmjs.org/',
      engineStrict: true,
      failOnMissingDependencies: true,
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: true,
      },
      prefix: process.cwd(),
      skipped: skippedPackages,
    },
  )

  t.deepEqual(filteredShrinkwrap, {
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
        }
      }
    },
    packages: {
      '/bar/1.0.0': {
        resolution: { integrity: '' },
      },
      '/dev-dep/1.0.0': {
        dev: true,
        resolution: { integrity: '' },
      },
      '/not-skipped-optional/1.0.0': {
        optional: true,
        resolution: { integrity: '' },
      },
      '/prod-dep-dep/1.0.0': {
        resolution: { integrity: '' },
      },
      '/prod-dep/1.0.0': {
        dependencies: {
          'bar': '1.0.0',
          'prod-dep-dep': '1.0.0',
        },
        optionalDependencies: {
          'optional-dep': '1.0.0',
        },
        resolution: { integrity: '' },
      },
    },
    shrinkwrapVersion: 4,
  })
  t.deepEqual(Array.from(skippedPackages), ['registry.npmjs.org/preserve-existing-skipped/1.0.0', 'registry.npmjs.org/optional-dep/1.0.0', 'registry.npmjs.org/foo/1.0.0'])
  t.end()
})
