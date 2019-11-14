import lockfileToAuditTree from '@pnpm/audit/lib/lockfileToAuditTree'
import test = require('tape')

test('lockfileToAuditTree()', (t) => {
  t.deepEqual(lockfileToAuditTree({
    importers: {
      '.': {
        dependencies: {
          foo: '1.0.0',
        },
        specifiers: {
          foo: '^1.0.0',
        },
      },
    },
    lockfileVersion: 5.1,
    packages: {
      '/bar/1.0.0': {
        resolution: {
          integrity: 'bar-integrity',
        },
      },
      '/foo/1.0.0': {
        dependencies: {
          bar: '1.0.0',
        },
        resolution: {
          integrity: 'foo-integrity',
        },
      },
    },
  }), {
    name: undefined,
    version: undefined,

    dependencies: {
      '.': {
        dependencies: {
          foo: {
            dependencies: {
              bar: {
                dependencies: {},
                dev: undefined,
                integrity: 'bar-integrity',
                requires: {},
                version: '1.0.0',
              },
            },
            dev: undefined,
            integrity: 'foo-integrity',
            requires: {
              bar: '1.0.0',
            },
            version: '1.0.0',
          },
        },
        requires: {
          foo: '1.0.0',
        },
        version: '0.0.0',
      },
    },
    dev: false,
    install: [],
    integrity: undefined,
    metadata: {},
    remove: [],
    requires: { '.': '0.0.0' },
  })
  t.end()
})
