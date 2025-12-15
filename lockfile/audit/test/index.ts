import path from 'path'
import { audit, bulkAudit } from '@pnpm/audit'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { type PnpmError } from '@pnpm/error'
import { fixtures } from '@pnpm/test-fixtures'
import { type DepPath, type ProjectId } from '@pnpm/types'
import nock from 'nock'
import { lockfileToAuditTree } from '../lib/lockfileToAuditTree.js'
import { lockfileToBulkAuditTree, type BulkAuditNode, type BulkAuditTree } from '../lib/lockfileToBulkAuditTree.js'
import { lockfileToPackageMap } from '../lib/lockfileToPackageMap.js'
import { readWantedLockfile } from '@pnpm/lockfile.fs'

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

  test('lockfileToBulkAuditTree()', async () => {
    const actual = await lockfileToBulkAuditTree({
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
    }, { lockfileDir: f.find('one-project') })

    const barNode: BulkAuditNode = {
      isDirect: false,
      name: 'bar',
      depPath: 'bar@1.0.0' as DepPath,
      version: '1.0.0',
      integrity: 'bar-integrity',
      dev: false,
      dependents: new Set(),
    }
    const fooNode: BulkAuditNode = {
      isDirect: true,
      name: 'foo',
      depPath: 'foo@1.0.0' as DepPath,
      version: '1.0.0',
      integrity: 'foo-integrity',
      dev: false,
      dependencies: {
        bar: barNode,
      },
      dependents: new Set(),
    }
    barNode.dependents.add(fooNode)
    const topLevelNode: BulkAuditNode = {
      name: '.',
      isImporter: true,
      isDirect: true,
      version: '1.0.0',
      dev: false,
      dependencies: {
        foo: fooNode,
      },
      dependents: new Set(),
    }
    fooNode.dependents.add(topLevelNode)
    const expected: BulkAuditTree = {
      importers: new Map<string, BulkAuditNode>([
        ['.', topLevelNode],
      ]),
      allNodesByPackageName: new Map<string, Set<BulkAuditNode>>([
        ['foo', new Set([fooNode])],
        ['bar', new Set([barNode])],
      ]),
    }

    // TODO: can't compare nodes directly because of circular references with dependents
    expect(new Set(actual.importers.keys())).toEqual(new Set(expected.importers.keys()))
    expect(new Set(actual.allNodesByPackageName.keys())).toEqual(new Set(expected.allNodesByPackageName.keys()))
  })

  test('lockfileToPackageMap()', () => {
    expect(lockfileToPackageMap({
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
    }, {})).toEqual(
      new Map<string, Set<string>>([
        ['foo', new Set(['1.0.0'])],
        ['bar', new Set(['1.0.0'])],
      ])
    )
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
    expect(err.message).toBe('The audit endpoint (at http://registry.registry/-/npm/v1/security/audits) responded with 500: {"message":"Something bad happened"}')
  })

  test('bulkAudit error', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    nock(registry, {
      badheaders: ['authorization'],
    })
      .post('/-/npm/v1/security/advisories/bulk')
      .reply(500, { message: 'Something bad happened' })

    let err!: PnpmError
    try {
      await bulkAudit({
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
        virtualStoreDir: 'node_modules/.pnpm',
        virtualStoreDirMaxLength: 120,
      })
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }

    expect(err).toBeDefined()
    expect(err.code).toBe('ERR_PNPM_AUDIT_BAD_RESPONSE')
    expect(err.message).toBe('The audit endpoint (at http://registry.registry/-/npm/v1/security/advisories/bulk) responded with 500: {"message":"Something bad happened"}')
  })

  test('bulkAudit-1', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    nock(registry, {
      badheaders: ['authorization'],
    })
      .post('/-/npm/v1/security/advisories/bulk')
      .reply(200, {
        'is-bigint': [
          { id: 123, url: 'https://example.com/vuln', title: 'vuln title', severity: 'critical', vulnerable_versions: '>=1.0.2 <1.0.4', cwe: ['CWE-330'], cvss: { score: 0, vectorString: null } },
        ],
      })

    const fixturePath = f.find('bulkAudit-1')

    const lockfile = await readWantedLockfile(fixturePath, {
      ignoreIncompatible: false,
    })
    expect(lockfile).not.toBeNull()

    const report = await bulkAudit(lockfile!,
      getAuthHeader,
      {
        lockfileDir: fixturePath,
        registry,
        retry: {
          retries: 0,
        },
        virtualStoreDir: path.join(fixturePath, 'node_modules/.pnpm'),
        virtualStoreDirMaxLength: 120,
      })

    expect(report.report.size).toBe(1)
    expect(report.report.has('is-bigint')).toBe(true)
    expect(report.report.get('is-bigint')!.fixAvailable).toBe(true)
  })

  test('bulkAudit-2', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    nock(registry, {
      badheaders: ['authorization'],
    })
      .post('/-/npm/v1/security/advisories/bulk')
      .reply(200, {
        'tar-fs': [
          { id: 123, url: 'https://example.com/vuln', title: 'vuln title', severity: 'critical', vulnerable_versions: '>=2.1.0 <2.1.4', cwe: ['CWE-330'], cvss: { score: 0, vectorString: null } },
        ],
      })

    const fixturePath = f.find('bulkAudit-2')

    const lockfile = await readWantedLockfile(fixturePath, {
      ignoreIncompatible: false,
    })
    expect(lockfile).not.toBeNull()

    const report = await bulkAudit(lockfile!,
      getAuthHeader,
      {
        lockfileDir: fixturePath,
        registry,
        retry: {
          retries: 0,
        },
        virtualStoreDir: path.join(fixturePath, 'node_modules/.pnpm'),
        virtualStoreDirMaxLength: 120,
      })

    expect(report.report.size).toBe(1)
    expect(report.report.has('tar-fs')).toBe(true)
    expect(report.report.get('tar-fs')!.fixAvailable).toBe(true)
  })
})
