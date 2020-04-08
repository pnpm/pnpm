import audit from '@pnpm/audit'
import lockfileToAuditTree from '@pnpm/audit/lib/lockfileToAuditTree'
import PnpmError from '@pnpm/error'
import nock = require('nock')
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
                dev: false,
                integrity: 'bar-integrity',
                version: '1.0.0',
              },
            },
            dev: false,
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

test('an error is thrown if the audit endpoint responds with a non-OK code', async (t) => {
  const registry = 'http://registry.registry/'
  nock(registry)
    .post('/-/npm/v1/security/audits')
    .reply(500, { message: 'Something bad happened' })

  let err!: PnpmError
  try {
    await audit({
      importers: {},
      lockfileVersion: 5,
    }, {
      registry,
      retry: {
        retries: 0,
      },
    })
  } catch (_err) {
    err = _err
  }

  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_AUDIT_BAD_RESPONSE')
  t.equal(err.message, 'The audit endpoint (at http://registry.registry/-/npm/v1/security/audits) responded with 500: {"message":"Something bad happened"}')
  t.end()
})
