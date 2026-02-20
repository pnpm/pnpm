import { audit } from '@pnpm/audit'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { type PnpmError } from '@pnpm/error'
import { fixtures } from '@pnpm/test-fixtures'
import { type DepPath, type ProjectId } from '@pnpm/types'
import nock from 'nock'
import { lockfileToAuditTree } from '../lib/lockfileToAuditTree.js'

const f = fixtures(import.meta.dirname)

describe('audit', () => {
  test('lockfileToAuditTree()', async () => {
    expect(await lockfileToAuditTree({
      importers: {
        ['.' as ProjectId]: {
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
        ['bar@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'bar-integrity',
          },
        },
        ['foo@1.0.0' as DepPath]: {
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
        ['.' as ProjectId]: {
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
        ['bar@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'bar-integrity',
          },
        },
        ['foo@1.0.0' as DepPath]: {
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
      .post('/-/npm/v1/security/audits/quick')
      .reply(500, { message: 'Something bad happened' })
    nock(registry, {
      badheaders: ['authorization'],
    })
      .post('/-/npm/v1/security/audits')
      .reply(500, { message: 'Fallback failed too' })

    let err!: PnpmError
    try {
      await audit({
        importers: {},
        lockfileVersion: LOCKFILE_VERSION,
      },
      getAuthHeader,
      {
        lockfileDir: f.find('one-project'),
        registry,
        retry: {
          retries: 0,
        },
        virtualStoreDirMaxLength: 120,
      })
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }

    expect(err).toBeDefined()
    expect(err.code).toBe('ERR_PNPM_AUDIT_BAD_RESPONSE')
    expect(err.message).toBe('The audit endpoint (at http://registry.registry/-/npm/v1/security/audits/quick) responded with 500: {"message":"Something bad happened"}. Fallback endpoint (at http://registry.registry/-/npm/v1/security/audits) responded with 500: {"message":"Fallback failed too"}')
  })

  test('falls back to /audits if /audits/quick fails', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    nock(registry, {
      badheaders: ['authorization'],
    })
      .post('/-/npm/v1/security/audits/quick')
      .reply(500, { message: 'Something bad happened' })
    nock(registry, {
      badheaders: ['authorization'],
    })
      .post('/-/npm/v1/security/audits')
      .reply(200, {
        actions: [],
        advisories: {},
        metadata: {
          dependencies: 0,
          devDependencies: 0,
          optionalDependencies: 0,
          totalDependencies: 0,
          vulnerabilities: {
            critical: 0,
            high: 0,
            info: 0,
            low: 0,
            moderate: 0,
          },
        },
        muted: [],
      })

    expect(await audit({
      importers: {},
      lockfileVersion: LOCKFILE_VERSION,
    },
    getAuthHeader,
    {
      lockfileDir: f.find('one-project'),
      registry,
      retry: {
        retries: 0,
      },
      virtualStoreDirMaxLength: 120,
    })).toEqual({
      actions: [],
      advisories: {},
      metadata: {
        dependencies: 0,
        devDependencies: 0,
        optionalDependencies: 0,
        totalDependencies: 0,
        vulnerabilities: {
          critical: 0,
          high: 0,
          info: 0,
          low: 0,
          moderate: 0,
        },
      },
      muted: [],
    })
  })
})
