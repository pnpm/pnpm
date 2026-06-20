import { describe, expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { audit, buildAuditPathIndex, lockfileToAuditRequest } from '@pnpm/deps.compliance.audit'
import type { PnpmError } from '@pnpm/error'
import type { PackageSnapshots } from '@pnpm/lockfile.types'
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

  test('buildAuditPathIndex() prunes non-vulnerable subtrees while enumerating paths', () => {
    let coldReads = 0
    const importers: Record<ProjectId, { dependencies: Record<string, string>, specifiers: Record<string, string> }> = {}
    const packages: PackageSnapshots = {
      ['vuln@1.0.0' as DepPath]: { resolution: { integrity: 'vuln-integrity' } },
    }
    Object.defineProperty(packages, 'cold@1.0.0', {
      enumerable: true,
      get: () => {
        coldReads++
        return { dependencies: { 'cold-leaf': '1.0.0' }, resolution: { integrity: 'cold-integrity' } }
      },
    })
    packages['cold-leaf@1.0.0' as DepPath] = { resolution: { integrity: 'cold-leaf-integrity' } }
    for (let i = 0; i < 50; i++) {
      const parentName = `parent-${i}`
      importers[`.${i}` as ProjectId] = {
        dependencies: { [parentName]: '1.0.0' },
        specifiers: { [parentName]: '1.0.0' },
      }
      packages[`${parentName}@1.0.0` as DepPath] = {
        dependencies: { cold: '1.0.0', vuln: '1.0.0' },
        resolution: { integrity: `${parentName}-integrity` },
      }
    }
    const result = buildAuditPathIndex({
      importers,
      lockfileVersion: LOCKFILE_VERSION,
      packages,
    }, new Set(['vuln']), { depTypes: {}, optionalOnly: new Set() })

    expect(result['vuln']!.get('1.0.0')!.paths).toHaveLength(50)
    expect(coldReads).toBe(1)
  })

  test('buildAuditPathIndex() stops reading saturated vulnerable nodes', () => {
    let vulnReads = 0
    const importers: Record<ProjectId, { dependencies: Record<string, string>, specifiers: Record<string, string> }> = {}
    const packages: PackageSnapshots = {}
    Object.defineProperty(packages, 'vuln@1.0.0', {
      enumerable: true,
      get: () => {
        vulnReads++
        return { resolution: { integrity: 'vuln-integrity' } }
      },
    })
    for (let i = 0; i < 150; i++) {
      importers[`.${i}` as ProjectId] = {
        dependencies: { vuln: '1.0.0' },
        specifiers: { vuln: '1.0.0' },
      }
    }
    const result = buildAuditPathIndex({
      importers,
      lockfileVersion: LOCKFILE_VERSION,
      packages,
    }, new Set(['vuln']), { depTypes: {}, optionalOnly: new Set() })

    expect(result['vuln']!.get('1.0.0')!.paths).toHaveLength(100)
    expect(vulnReads).toBe(101)
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

  test('buildAuditPathIndex() preserves reachability across cyclic dependencies', () => {
    const lockfile = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: { a: '1.0.0' },
          specifiers: { a: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['a@1.0.0' as DepPath]: {
          dependencies: { b: '1.0.0' },
          resolution: { integrity: 'a-integrity' },
        },
        ['b@1.0.0' as DepPath]: {
          dependencies: { a: '1.0.0' },
          resolution: { integrity: 'b-integrity' },
        },
      } as PackageSnapshots,
    }
    const result = buildAuditPathIndex(lockfile, new Set(['a', 'b']), {})

    expect(result['a']!.get('1.0.0')).toEqual({
      paths: ['.>a'],
      dev: false,
      optional: false,
    })
    expect(result['b']!.get('1.0.0')).toEqual({
      paths: ['.>a>b'],
      dev: false,
      optional: false,
    })
  })

  test('buildAuditPathIndex() preserves vulnerability reachability when cycle root is queried second', () => {
    const lockfile = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: { root: '1.0.0' },
          specifiers: { root: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['root@1.0.0' as DepPath]: {
          dependencies: { b: '1.0.0' },
          resolution: { integrity: 'root-integrity' },
        },
        ['a@1.0.0' as DepPath]: {
          dependencies: { b: '1.0.0' },
          resolution: { integrity: 'a-integrity' },
        },
        ['b@1.0.0' as DepPath]: {
          dependencies: { a: '1.0.0' },
          resolution: { integrity: 'b-integrity' },
        },
      } as PackageSnapshots,
    }
    const result = buildAuditPathIndex(lockfile, new Set(['a']), {})

    expect(result['a']!.get('1.0.0')).toEqual({
      paths: ['.>root>b>a'],
      dev: false,
      optional: false,
    })
  })

  test('buildAuditPathIndex() keeps paths reached through a non-entry cycle member', () => {
    // The importer reaches the b<->c cycle through both c and b. Visiting c
    // first must not memoize an under-approximated reachable set for b that
    // would prune the still-valid `.>b>c>x` path to the vulnerable package.
    const lockfile = {
      importers: {
        ['.' as ProjectId]: {
          dependencies: { c: '1.0.0', b: '1.0.0' },
          specifiers: { c: '^1.0.0', b: '^1.0.0' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        ['b@1.0.0' as DepPath]: {
          dependencies: { c: '1.0.0' },
          resolution: { integrity: 'b-integrity' },
        },
        ['c@1.0.0' as DepPath]: {
          dependencies: { b: '1.0.0', x: '1.0.0' },
          resolution: { integrity: 'c-integrity' },
        },
        ['x@1.0.0' as DepPath]: {
          resolution: { integrity: 'x-integrity' },
        },
      } as PackageSnapshots,
    }
    const result = buildAuditPathIndex(lockfile, new Set(['x']), {})

    expect(result['x']!.get('1.0.0')!.paths.sort()).toEqual(['.>b>c>x', '.>c>x'])
  })

  test('buildAuditPathIndex() scales linearly, not quadratically, with cycle size', () => {
    // n0 -> ... -> n(L-1) -> n0 is one big cycle with a single vulnerable leaf.
    // Recomputing the cycle for every ancestor scans ~L^2 nodes, so doubling L
    // quadruples the reads; linear work only doubles them. Comparing the growth
    // rate of two sizes — rather than an absolute count — keeps the assertion
    // robust to constant-factor changes in future refactors.
    const countReads = (L: number): number => {
      let reads = 0
      const importers = {
        ['.' as ProjectId]: { dependencies: { n0: '1.0.0' }, specifiers: { n0: '^1.0.0' } },
      }
      const packages: PackageSnapshots = {}
      for (let i = 0; i < L; i++) {
        const nextName = i + 1 < L ? `n${i + 1}` : 'n0' // the last node loops back to n0
        const deps = { [nextName]: '1.0.0', [`leaf${i}`]: '1.0.0' }
        Object.defineProperty(packages, `n${i}@1.0.0`, {
          enumerable: true,
          get: () => {
            reads++
            return { dependencies: deps, resolution: { integrity: `n${i}-integrity` } }
          },
        })
        packages[`leaf${i}@1.0.0` as DepPath] = { resolution: { integrity: `leaf${i}-integrity` } }
      }

      const result = buildAuditPathIndex({
        importers,
        lockfileVersion: LOCKFILE_VERSION,
        packages,
      }, new Set(['leaf0']), { depTypes: {}, optionalOnly: new Set() })

      // Correctness: the only vulnerable leaf is still reported with its path.
      expect(result['leaf0']!.get('1.0.0')).toEqual({
        paths: ['.>n0>leaf0'],
        dev: false,
        optional: false,
      })
      return reads
    }

    // Linear ⇒ ratio ≈ 2; quadratic ⇒ ratio ≈ 4. Assert clearly sub-quadratic.
    expect(countReads(400) / countReads(200)).toBeLessThan(3)
  })

  test('buildAuditPathIndex() handles a very deep dependency chain without overflowing the stack', () => {
    // n0 -> n1 -> ... -> n(L-1) -> vuln is a single chain far deeper than the JS
    // call-stack limit. A recursive walk (reachability or path traversal) would
    // throw RangeError on this lockfile (a lockfile is untrusted input); the
    // iterative implementation must complete and still report the leaf.
    const L = 60_000
    const packages: PackageSnapshots = {}
    for (let i = 0; i < L; i++) {
      const child = i + 1 < L ? `n${i + 1}` : 'vuln'
      packages[`n${i}@1.0.0` as DepPath] = { dependencies: { [child]: '1.0.0' }, resolution: { integrity: `n${i}-integrity` } }
    }
    packages['vuln@1.0.0' as DepPath] = { resolution: { integrity: 'vuln-integrity' } }

    const result = buildAuditPathIndex({
      importers: { ['.' as ProjectId]: { dependencies: { n0: '1.0.0' }, specifiers: { n0: '^1.0.0' } } },
      lockfileVersion: LOCKFILE_VERSION,
      packages,
    }, new Set(['vuln']), { depTypes: {}, optionalOnly: new Set() })

    const info = result['vuln']!.get('1.0.0')!
    expect(info.paths).toHaveLength(1)
    expect(info.paths[0].startsWith('.>n0>n1>')).toBe(true)
    expect(info.paths[0].endsWith('>vuln')).toBe(true)
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
