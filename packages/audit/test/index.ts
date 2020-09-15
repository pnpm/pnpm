import audit from '@pnpm/audit'
import lockfileToAuditTree from '@pnpm/audit/lib/lockfileToAuditTree'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import nock = require('nock')

describe('audit', () => {
  test('lockfileToAuditTree()', () => {
    expect(lockfileToAuditTree({
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
      lockfileVersion: LOCKFILE_VERSION,
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
    })).toEqual({
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
  })

  test('an error is thrown if the audit endpoint responds with a non-OK code', async () => {
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

    expect(err).toBeDefined()
    expect(err.code).toEqual('ERR_PNPM_AUDIT_BAD_RESPONSE')
    expect(err.message).toEqual('The audit endpoint (at http://registry.registry/-/npm/v1/security/audits) responded with 500: {"message":"Something bad happened"}')
  })
})
