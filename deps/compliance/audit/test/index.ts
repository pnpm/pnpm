import { audit } from '@pnpm/audit'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import type { PnpmError } from '@pnpm/error'
import { fixtures } from '@pnpm/test-fixtures'
import type { DepPath, ProjectId } from '@pnpm/types'
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

  test('lockfileToAuditTree() includes env lockfile configDependencies and packageManagerDependencies as separate groups', async () => {
    const result = await lockfileToAuditTree({
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
        ['foo@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'foo-integrity',
          },
        },
      },
    }, {
      envLockfile: {
        lockfileVersion: LOCKFILE_VERSION,
        importers: {
          '.': {
            configDependencies: {
              'my-config': {
                specifier: '2.0.0',
                version: '2.0.0',
              },
            },
            packageManagerDependencies: {
              pnpm: {
                specifier: '9.0.0',
                version: '9.0.0',
              },
            },
          },
        },
        packages: {
          'my-config@2.0.0': {
            resolution: { integrity: 'my-config-integrity' },
          },
          'config-util@1.0.0': {
            resolution: { integrity: 'config-util-integrity' },
          },
          'pnpm@9.0.0': {
            resolution: { integrity: 'pnpm-integrity' },
          },
        },
        snapshots: {
          'my-config@2.0.0': {
            dependencies: {
              'config-util': '1.0.0',
            },
          },
          'config-util@1.0.0': {},
          'pnpm@9.0.0': {},
        },
      },
      lockfileDir: f.find('one-project'),
    })

    expect(result.dependencies).toHaveProperty('configDependencies')
    expect(result.dependencies).toHaveProperty('packageManagerDependencies')

    expect(result.dependencies!['configDependencies']).toEqual({
      dev: false,
      version: '0.0.0',
      dependencies: {
        'my-config': {
          dev: false,
          integrity: 'my-config-integrity',
          version: '2.0.0',
          dependencies: {
            'config-util': {
              dev: false,
              integrity: 'config-util-integrity',
              version: '1.0.0',
            },
          },
          requires: {
            'config-util': '1.0.0',
          },
        },
      },
      requires: {
        'my-config': '2.0.0',
      },
    })

    expect(result.dependencies!['packageManagerDependencies']).toEqual({
      dev: false,
      version: '0.0.0',
      dependencies: {
        pnpm: {
          dev: false,
          integrity: 'pnpm-integrity',
          version: '9.0.0',
        },
      },
      requires: {
        pnpm: '9.0.0',
      },
    })
  })

  test('lockfileToAuditTree() with env lockfile with only configDependencies omits packageManagerDependencies group', async () => {
    const result = await lockfileToAuditTree({
      importers: {
        ['.' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    }, {
      envLockfile: {
        lockfileVersion: LOCKFILE_VERSION,
        importers: {
          '.': {
            configDependencies: {
              'my-hook': {
                specifier: '1.0.0',
                version: '1.0.0',
              },
            },
          },
        },
        packages: {
          'my-hook@1.0.0': {
            resolution: { integrity: 'my-hook-integrity' },
          },
        },
        snapshots: {
          'my-hook@1.0.0': {},
        },
      },
      lockfileDir: f.find('one-project'),
    })

    expect(result.dependencies).toHaveProperty('configDependencies')
    expect(result.dependencies).not.toHaveProperty('packageManagerDependencies')
  })

  test('lockfileToAuditTree() with env lockfile with empty configDependencies and no packageManagerDependencies adds no groups', async () => {
    const result = await lockfileToAuditTree({
      importers: {
        ['.' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    }, {
      envLockfile: {
        lockfileVersion: LOCKFILE_VERSION,
        importers: {
          '.': {
            configDependencies: {},
          },
        },
        packages: {},
        snapshots: {},
      },
      lockfileDir: f.find('one-project'),
    })

    expect(result.dependencies).not.toHaveProperty('configDependencies')
    expect(result.dependencies).not.toHaveProperty('packageManagerDependencies')
  })

  test('lockfileToAuditTree() with null envLockfile adds no groups', async () => {
    const result = await lockfileToAuditTree({
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
        ['foo@1.0.0' as DepPath]: {
          resolution: {
            integrity: 'foo-integrity',
          },
        },
      },
    }, {
      envLockfile: null,
      lockfileDir: f.find('one-project'),
    })

    expect(result.dependencies).not.toHaveProperty('configDependencies')
    expect(result.dependencies).not.toHaveProperty('packageManagerDependencies')
    expect(result.dependencies!['.'] ).toBeDefined()
  })

  test('lockfileToAuditTree() env lockfile includes optionalDependencies from snapshots', async () => {
    const result = await lockfileToAuditTree({
      importers: {
        ['.' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    }, {
      envLockfile: {
        lockfileVersion: LOCKFILE_VERSION,
        importers: {
          '.': {
            configDependencies: {
              'my-tool': {
                specifier: '1.0.0',
                version: '1.0.0',
              },
            },
          },
        },
        packages: {
          'my-tool@1.0.0': {
            resolution: { integrity: 'my-tool-integrity' },
          },
          'required-dep@1.0.0': {
            resolution: { integrity: 'required-dep-integrity' },
          },
          'optional-dep@2.0.0': {
            resolution: { integrity: 'optional-dep-integrity' },
          },
        },
        snapshots: {
          'my-tool@1.0.0': {
            dependencies: {
              'required-dep': '1.0.0',
            },
            optionalDependencies: {
              'optional-dep': '2.0.0',
            },
          },
          'required-dep@1.0.0': {},
          'optional-dep@2.0.0': {},
        },
      },
      lockfileDir: f.find('one-project'),
    })

    const myTool = result.dependencies!['configDependencies']?.dependencies!['my-tool']
    expect(myTool).toBeDefined()
    expect(myTool.dependencies).toHaveProperty('required-dep')
    expect(myTool.dependencies).toHaveProperty('optional-dep')
    expect(myTool.dependencies!['required-dep']).toEqual({
      dev: false,
      integrity: 'required-dep-integrity',
      version: '1.0.0',
    })
    expect(myTool.dependencies!['optional-dep']).toEqual({
      dev: false,
      integrity: 'optional-dep-integrity',
      version: '2.0.0',
    })
    expect(myTool.requires).toEqual({
      'required-dep': '1.0.0',
      'optional-dep': '2.0.0',
    })
  })

  test('lockfileToAuditTree() env lockfile does not include unreachable packages', async () => {
    const result = await lockfileToAuditTree({
      importers: {
        ['.' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    }, {
      envLockfile: {
        lockfileVersion: LOCKFILE_VERSION,
        importers: {
          '.': {
            configDependencies: {
              'my-config': {
                specifier: '1.0.0',
                version: '1.0.0',
              },
            },
          },
        },
        packages: {
          'my-config@1.0.0': {
            resolution: { integrity: 'my-config-integrity' },
          },
          'orphan-pkg@3.0.0': {
            resolution: { integrity: 'orphan-integrity' },
          },
        },
        snapshots: {
          'my-config@1.0.0': {},
          'orphan-pkg@3.0.0': {},
        },
      },
      lockfileDir: f.find('one-project'),
    })

    const configDeps = result.dependencies!['configDependencies']
    expect(configDeps.dependencies).toHaveProperty('my-config')
    expect(configDeps.dependencies).not.toHaveProperty('orphan-pkg')

    // Also verify it doesn't appear anywhere in the top-level dependencies
    expect(result.dependencies).not.toHaveProperty('orphan-pkg')
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
