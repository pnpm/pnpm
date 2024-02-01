import { audit } from '@pnpm/audit'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { type PnpmError } from '@pnpm/error'
import { fixtures } from '@pnpm/test-fixtures'
import nock from 'nock'
import { lockfileToAuditTree } from '../lib/lockfileToAuditTree'

const f = fixtures(__dirname)

describe('audit', () => {
  test('lockfileToAuditTree()', async () => {
    expect(await lockfileToAuditTree({
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
        '/bar@1.0.0': {
          resolution: {
            integrity: 'bar-integrity',
          },
        },
        '/foo@1.0.0': {
          dependencies: {
            bar: '1.0.0',
          },
          resolution: {
            integrity: 'foo-integrity',
          },
        },
      },
    }, { lockfileDir: f.find('one-project') })).toEqual({
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
          dev: false,
          requires: {
            foo: '1.0.0',
          },
          version: '1.0.0',
        },
      },
      dev: false,
      install: [],
      integrity: undefined,
      metadata: {},
      remove: [],
      requires: { '.': '1.0.0' },
    })
  })

  test('lockfileToAuditTree() without specified version should use default version 0.0.0', async () => {
    expect(await lockfileToAuditTree({
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
        '/bar@1.0.0': {
          resolution: {
            integrity: 'bar-integrity',
          },
        },
        '/foo@1.0.0': {
          dependencies: {
            bar: '1.0.0',
          },
          resolution: {
            integrity: 'foo-integrity',
          },
        },
      },
    }, { lockfileDir: f.find('project-without-version') })).toEqual({
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
          dev: false,
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
    const getAuthHeader = () => undefined
    nock(registry, {
      badheaders: ['authorization'],
    })
      .post('/-/npm/v1/security/audits')
      .reply(500, { message: 'Something bad happened' })

    let err!: PnpmError
    try {
      await audit({
        importers: {},
        lockfileVersion: 5,
      },
      getAuthHeader,
      {
        lockfileDir: f.find('one-project'),
        registry,
        retry: {
          retries: 0,
        },
      })
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }

    expect(err).toBeDefined()
    expect(err.code).toEqual('ERR_PNPM_AUDIT_BAD_RESPONSE')
    expect(err.message).toEqual('The audit endpoint (at http://registry.registry/-/npm/v1/security/audits) responded with 500: {"message":"Something bad happened"}')
  })
})
