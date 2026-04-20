import { LOCKFILE_VERSION } from '@pnpm/constants'
import { audit, buildAuditPathIndex, lockfileToAuditRequest } from '@pnpm/deps.compliance.audit'
import type { PnpmError } from '@pnpm/error'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import type { DepPath, ProjectId } from '@pnpm/types'

describe('audit', () => {
  test('lockfileToAuditRequest() flattens dependencies', () => {
    const result = lockfileToAuditRequest({
      importers: {
        ['.' as ProjectId]: {
          dependencies: { foo: '1.0.0' },
          specifiers: { foo: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['bar@1.0.0' as DepPath]: { resolution: { integrity: 'bar-integrity' } },
        ['foo@1.0.0' as DepPath]: {
          dependencies: { bar: '1.0.0' },
          resolution: { integrity: 'foo-integrity' },
        },
      },
    }, {})

    expect(result.request).toEqual({ foo: ['1.0.0'], bar: ['1.0.0'] })
    expect(result.totalDependencies).toBe(2)
    expect(result.devDependencies).toBe(0)
  })

  test('buildAuditPathIndex() records install paths for vulnerable packages', () => {
    const lockfile = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: { foo: '1.0.0' },
          specifiers: { foo: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['bar@1.0.0' as DepPath]: { resolution: { integrity: 'bar-integrity' } },
        ['foo@1.0.0' as DepPath]: {
          dependencies: { bar: '1.0.0' },
          resolution: { integrity: 'foo-integrity' },
        },
      },
    }
    const result = buildAuditPathIndex(lockfile, new Set(['bar']), {})

    expect(result['bar']!.get('1.0.0')).toEqual({ paths: ['.>foo>bar'], dev: false, optional: false })
    expect(result['foo']).toBeUndefined()
  })

  test('buildAuditPathIndex() records every distinct install path for shared deps', () => {
    // lodash is reachable via two different parent chains. The lockfile walker
    // globally dedupes by depPath, so using it directly would record only the
    // first-seen chain. buildAuditPathIndex must produce one path per chain.
    const lockfile = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: { a: '1.0.0', b: '1.0.0' },
          specifiers: { a: '^1.0.0', b: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['a@1.0.0' as DepPath]: {
          dependencies: { lodash: '4.0.0' },
          resolution: { integrity: 'a-integrity' },
        },
        ['b@1.0.0' as DepPath]: {
          dependencies: { lodash: '4.0.0' },
          resolution: { integrity: 'b-integrity' },
        },
        ['lodash@4.0.0' as DepPath]: { resolution: { integrity: 'lodash-integrity' } },
      },
    }
    const result = buildAuditPathIndex(lockfile, new Set(['lodash']), {})

    const info = result['lodash']!.get('4.0.0')!
    expect(info.paths).toHaveLength(2)
    expect(info.paths).toEqual(expect.arrayContaining(['.>a>lodash', '.>b>lodash']))
  })

  test('buildAuditPathIndex() classifies as optional when the only non-optional path runs through an excluded devDependency', () => {
    // shared-pkg is reachable two ways: via a devDependency chain (excluded
    // when include.devDependencies === false) and via an optionalDependency
    // root. With dev excluded, the only remaining path runs through the
    // optional edge, so the finding should be flagged as optional.
    const lockfile = {
      importers: {
        ['.' as ProjectId]: {
          devDependencies: { 'dev-root': '1.0.0' },
          optionalDependencies: { 'opt-root': '1.0.0' },
          specifiers: { 'dev-root': '^1.0.0', 'opt-root': '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['dev-root@1.0.0' as DepPath]: {
          dependencies: { 'shared-pkg': '1.0.0' },
          resolution: { integrity: 'dev-root-integrity' },
        },
        ['opt-root@1.0.0' as DepPath]: {
          dependencies: { 'shared-pkg': '1.0.0' },
          resolution: { integrity: 'opt-root-integrity' },
        },
        ['shared-pkg@1.0.0' as DepPath]: { resolution: { integrity: 'shared-pkg-integrity' } },
      },
    }

    const withDev = buildAuditPathIndex(lockfile, new Set(['shared-pkg']), {
      include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    })
    // When the dev chain is in scope the dep is reachable via a non-optional
    // path too, so it is NOT optional-only.
    expect(withDev['shared-pkg']!.get('1.0.0')!.optional).toBe(false)

    const prodOnly = buildAuditPathIndex(lockfile, new Set(['shared-pkg']), {
      include: { dependencies: true, devDependencies: false, optionalDependencies: true },
    })
    // With devDependencies excluded the only remaining way to reach shared-pkg
    // is through opt-root, so the dep becomes optional-only.
    expect(prodOnly['shared-pkg']!.get('1.0.0')!.optional).toBe(true)
  })

  test('buildAuditPathIndex() flags findings reached only through optional edges', () => {
    const lockfile = {
      importers: {
        ['.' as ProjectId]: {
          optionalDependencies: { native: '1.0.0' },
          specifiers: { native: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['native@1.0.0' as DepPath]: { resolution: { integrity: 'native-integrity' } },
      },
    }
    const result = buildAuditPathIndex(lockfile, new Set(['native']), {})

    expect(result['native']!.get('1.0.0')).toEqual({
      paths: ['.>native'],
      dev: false,
      optional: true,
    })
  })

  test('buildAuditPathIndex() replaces slashes in workspace importer ids', () => {
    const lockfile = {
      importers: {
        ['packages/foo' as ProjectId]: {
          dependencies: { foo: '1.0.0' },
          specifiers: { foo: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'foo-integrity' } },
      },
    }
    const result = buildAuditPathIndex(lockfile, new Set(['foo']), {})

    expect(result['foo']!.get('1.0.0')!.paths).toEqual(['packages__foo>foo'])
  })

  test('lockfileToAuditRequest() includes env lockfile configDependencies and packageManagerDependencies', () => {
    const result = lockfileToAuditRequest({
      importers: {
        ['.' as ProjectId]: {
          dependencies: { foo: '1.0.0' },
          specifiers: { foo: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'foo-integrity' } },
      },
    }, {
      envLockfile: {
        lockfileVersion: LOCKFILE_VERSION,
        importers: {
          '.': {
            configDependencies: {
              'my-config': { specifier: '2.0.0', version: '2.0.0' },
            },
            packageManagerDependencies: {
              pnpm: { specifier: '9.0.0', version: '9.0.0' },
            },
          },
        },
        packages: {
          'my-config@2.0.0': { resolution: { integrity: 'my-config-integrity' } },
          'config-util@1.0.0': { resolution: { integrity: 'config-util-integrity' } },
          'pnpm@9.0.0': { resolution: { integrity: 'pnpm-integrity' } },
        },
        snapshots: {
          'my-config@2.0.0': { dependencies: { 'config-util': '1.0.0' } },
          'config-util@1.0.0': {},
          'pnpm@9.0.0': {},
        },
      },
    })

    expect(result.request['foo']).toEqual(['1.0.0'])
    expect(result.request['my-config']).toEqual(['2.0.0'])
    expect(result.request['config-util']).toEqual(['1.0.0'])
    expect(result.request['pnpm']).toEqual(['9.0.0'])
  })

  test('lockfileToAuditRequest() accepts a null envLockfile', () => {
    const result = lockfileToAuditRequest({
      importers: {
        ['.' as ProjectId]: {
          dependencies: { foo: '1.0.0' },
          specifiers: { foo: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'foo-integrity' } },
      },
    }, { envLockfile: null })

    expect(result.request).toEqual({ foo: ['1.0.0'] })
  })

  test('lockfileToAuditRequest() includes optionalDependencies from env snapshots', () => {
    const result = lockfileToAuditRequest({
      importers: {
        ['.' as ProjectId]: { specifiers: {} },
      },
      lockfileVersion: LOCKFILE_VERSION,
    }, {
      envLockfile: {
        lockfileVersion: LOCKFILE_VERSION,
        importers: {
          '.': {
            configDependencies: {
              'my-tool': { specifier: '1.0.0', version: '1.0.0' },
            },
          },
        },
        packages: {
          'my-tool@1.0.0': { resolution: { integrity: 'my-tool-integrity' } },
          'required-dep@1.0.0': { resolution: { integrity: 'required-dep-integrity' } },
          'optional-dep@2.0.0': { resolution: { integrity: 'optional-dep-integrity' } },
        },
        snapshots: {
          'my-tool@1.0.0': {
            dependencies: { 'required-dep': '1.0.0' },
            optionalDependencies: { 'optional-dep': '2.0.0' },
          },
          'required-dep@1.0.0': {},
          'optional-dep@2.0.0': {},
        },
      },
    })

    expect(result.request['required-dep']).toEqual(['1.0.0'])
    expect(result.request['optional-dep']).toEqual(['2.0.0'])
  })

  test('lockfileToAuditRequest() does not include env packages unreachable from importers', () => {
    const result = lockfileToAuditRequest({
      importers: {
        ['.' as ProjectId]: { specifiers: {} },
      },
      lockfileVersion: LOCKFILE_VERSION,
    }, {
      envLockfile: {
        lockfileVersion: LOCKFILE_VERSION,
        importers: {
          '.': {
            configDependencies: {
              'my-config': { specifier: '1.0.0', version: '1.0.0' },
            },
          },
        },
        packages: {
          'my-config@1.0.0': { resolution: { integrity: 'my-config-integrity' } },
          'orphan-pkg@3.0.0': { resolution: { integrity: 'orphan-integrity' } },
        },
        snapshots: {
          'my-config@1.0.0': {},
          'orphan-pkg@3.0.0': {},
        },
      },
    })

    expect(result.request).toHaveProperty('my-config')
    expect(result.request).not.toHaveProperty('orphan-pkg')
  })

  test('an error is thrown if the audit endpoint responds with a non-OK code', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    await setupMockAgent()
    getMockAgent().get('http://registry.registry')
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(500, { message: 'Something bad happened' })

    try {
      let err!: PnpmError
      try {
        await audit({
          importers: {},
          lockfileVersion: LOCKFILE_VERSION,
        },
        getAuthHeader,
        {
          registry,
          retry: {
            retries: 0,
          },
        })
      } catch (_err: any) { // eslint-disable-line
        err = _err
      }

      expect(err).toBeDefined()
      expect(err.code).toBe('ERR_PNPM_AUDIT_BAD_RESPONSE')
      expect(err.message).toBe('The audit endpoint (at http://registry.registry/-/npm/v1/security/advisories/bulk) responded with 500: {"message":"Something bad happened"}')
    } finally {
      await teardownMockAgent()
    }
  })

  test('throws AUDIT_BAD_RESPONSE if the registry body is not valid JSON', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    await setupMockAgent()
    getMockAgent().get('http://registry.registry')
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, 'not json <html>')

    try {
      let err!: PnpmError
      try {
        await audit(
          { importers: {}, lockfileVersion: LOCKFILE_VERSION },
          getAuthHeader,
          { registry, retry: { retries: 0 } }
        )
      } catch (_err: any) { // eslint-disable-line
        err = _err
      }
      expect(err).toBeDefined()
      expect(err.code).toBe('ERR_PNPM_AUDIT_BAD_RESPONSE')
      expect(err.message).toMatch(/invalid JSON/)
    } finally {
      await teardownMockAgent()
    }
  })

  test('throws AUDIT_BAD_RESPONSE if the registry returns a non-object body', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    await setupMockAgent()
    getMockAgent().get('http://registry.registry')
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, [])

    try {
      let err!: PnpmError
      try {
        await audit(
          { importers: {}, lockfileVersion: LOCKFILE_VERSION },
          getAuthHeader,
          { registry, retry: { retries: 0 } }
        )
      } catch (_err: any) { // eslint-disable-line
        err = _err
      }
      expect(err).toBeDefined()
      expect(err.code).toBe('ERR_PNPM_AUDIT_BAD_RESPONSE')
      expect(err.message).toMatch(/unexpected body/)
    } finally {
      await teardownMockAgent()
    }
  })

  test('sends authorization header when getAuthHeader returns a value', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => 'Bearer test-token'
    await setupMockAgent()
    // intercept will only match if the authorization header is present and correct
    getMockAgent().get('http://registry.registry')
      .intercept({
        path: '/-/npm/v1/security/advisories/bulk',
        method: 'POST',
        headers: { authorization: 'Bearer test-token' },
      })
      .reply(200, {})

    try {
      const result = await audit(
        { importers: {}, lockfileVersion: LOCKFILE_VERSION },
        getAuthHeader,
        { registry, retry: { retries: 0 } }
      )
      expect(result.advisories).toEqual({})
    } finally {
      await teardownMockAgent()
    }
  })

  test('computes findings paths and severity counts locally when the bulk response omits findings', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    await setupMockAgent()
    // Bare bulk response — no `findings`, no `patched_versions`, no `cves`,
    // no `module_name`. Exactly what registry.npmjs.org returns today.
    getMockAgent().get('http://registry.registry')
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, {
        bar: [
          {
            id: 42,
            url: 'https://github.com/advisories/GHSA-xxxx-yyyy-zzzz',
            title: 'bar is bad',
            severity: 'high',
            vulnerable_versions: '<2.0.0',
          },
        ],
      })

    try {
      const result = await audit(
        {
          importers: {
            ['.' as ProjectId]: {
              dependencies: { foo: '1.0.0' },
              specifiers: { foo: '^1.0.0' },
            },
          },
          lockfileVersion: LOCKFILE_VERSION,
          packages: {
            ['bar@1.0.0' as DepPath]: { resolution: { integrity: 'bar-integrity' } },
            ['foo@1.0.0' as DepPath]: {
              dependencies: { bar: '1.0.0' },
              resolution: { integrity: 'foo-integrity' },
            },
          },
        },
        getAuthHeader,
        { registry, retry: { retries: 0 } }
      )
      const advisory = result.advisories['42']
      expect(advisory).toBeDefined()
      expect(advisory.module_name).toBe('bar')
      expect(advisory.github_advisory_id).toBe('GHSA-xxxx-yyyy-zzzz')
      expect(advisory.patched_versions).toBe('>=2.0.0')
      expect(advisory.findings).toHaveLength(1)
      expect(advisory.findings[0].version).toBe('1.0.0')
      expect(advisory.findings[0].paths).toEqual(['.>foo>bar'])
      expect(result.metadata.vulnerabilities.high).toBe(1)
      expect(result.metadata.totalDependencies).toBe(2)
    } finally {
      await teardownMockAgent()
    }
  })

  test('does not send authorization header when getAuthHeader returns undefined', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    await setupMockAgent()
    let capturedHeaders: Record<string, string> = {}
    getMockAgent().get('http://registry.registry')
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, (opts) => {
        capturedHeaders = opts.headers as Record<string, string>
        return {}
      })

    try {
      await audit(
        { importers: {}, lockfileVersion: LOCKFILE_VERSION },
        getAuthHeader,
        { registry, retry: { retries: 0 } }
      )
      expect(capturedHeaders).not.toHaveProperty('authorization')
    } finally {
      await teardownMockAgent()
    }
  })

  test('handles info severity in bulk response', async () => {
    const registry = 'http://registry.registry/'
    const getAuthHeader = () => undefined
    await setupMockAgent()
    getMockAgent().get('http://registry.registry')
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, {
        info_pkg: [
          {
            id: 100,
            url: 'https://github.com/advisories/GHSA-info-info-info',
            title: 'just some info',
            severity: 'info',
            vulnerable_versions: '*',
          },
        ],
      })

    try {
      const result = await audit(
        {
          importers: {
            ['.' as ProjectId]: {
              dependencies: { info_pkg: '1.0.0' },
              specifiers: { info_pkg: '1.0.0' },
            },
          },
          lockfileVersion: LOCKFILE_VERSION,
          packages: {
            ['info_pkg@1.0.0' as DepPath]: { resolution: { integrity: 'info-integrity' } },
          },
        },
        getAuthHeader,
        { registry, retry: { retries: 0 } }
      )
      expect(result.metadata.vulnerabilities.info).toBe(1)
      expect(result.advisories['100'].severity).toBe('info')
    } finally {
      await teardownMockAgent()
    }
  })
})
