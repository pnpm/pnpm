import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { AuditEndpointNotExistsError } from '@pnpm/audit'
import { audit } from '@pnpm/plugin-commands-audit'
import { install } from '@pnpm/plugin-commands-installation'
import { fixtures } from '@pnpm/test-fixtures'
import nock from 'nock'

import { AUDIT_REGISTRY, AUDIT_REGISTRY_OPTS, DEFAULT_OPTS } from './utils/options.js'
import * as responses from './utils/responses/index.js'

const f = fixtures(path.join(import.meta.dirname, 'fixtures'))

describe('plugin-commands-audit', () => {
  const hasVulnerabilitiesDir = f.prepare('has-vulnerabilities')
  beforeAll(async () => {
    await install.handler({
      ...DEFAULT_OPTS,
      frozenLockfile: true,
      dir: hasVulnerabilitiesDir,
    })
  })
  afterEach(() => {
    nock.cleanAll()
  })
  test('audit', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.ALL_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
    })
    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit --dev', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      dev: true,
      production: false,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit --audit-level', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.ALL_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'moderate',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit: no vulnerabilities', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.NO_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
    })

    expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
    expect(exitCode).toBe(0)
  })

  test('audit --json', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.ALL_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      json: true,
    })

    const json = JSON.parse(output)
    expect(json.metadata).toBeTruthy()
    expect(exitCode).toBe(1)
  })

  test.skip('audit does not exit with code 1 if the found vulnerabilities are having lower severity then what we asked for', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'high',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      dev: true,
    })

    expect(exitCode).toBe(0)
    expect(stripAnsi(output)).toBe(`1 vulnerabilities found
  Severity: 1 moderate`)
  })

  test('audit --json respects audit-level', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { exitCode, output } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'critical',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      json: true,
      dev: true,
    })

    expect(exitCode).toBe(0)
    const parsed = JSON.parse(output)
    expect(Object.keys(parsed.advisories)).toHaveLength(0)
  })

  test('audit --json filters advisories by audit-level', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { exitCode, output } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'high',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      json: true,
      dev: true,
    })

    expect(exitCode).toBe(1)
    const parsed = JSON.parse(output)
    // DEV_VULN_ONLY_RESP has 4 high and 2 moderate advisories
    // With audit-level=high, only the 4 high advisories should be included
    expect(Object.keys(parsed.advisories)).toHaveLength(4)
    for (const advisory of Object.values(parsed.advisories) as Array<{ severity: string }>) {
      expect(advisory.severity).toBe('high')
    }
  })

  test('audit does not exit with code 1 if the registry responds with a non-200 response and ignoreRegistryErrors is used', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(500, { message: 'Something bad happened' })
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits')
      .reply(500, { message: 'Fallback failed too' })
    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      dev: true,
      fetchRetries: 0,
      ignoreRegistryErrors: true,
      production: false,
    })

    expect(exitCode).toBe(0)
    expect(stripAnsi(output)).toBe(`The audit endpoint (at ${AUDIT_REGISTRY}-/npm/v1/security/audits/quick) responded with 500: {"message":"Something bad happened"}. Fallback endpoint (at ${AUDIT_REGISTRY}-/npm/v1/security/audits) responded with 500: {"message":"Fallback failed too"}`)
  })

  test('audit sends authToken', async () => {
    nock(AUDIT_REGISTRY, {
      reqheaders: { authorization: 'Bearer 123' },
    })
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.NO_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      rawConfig: {
        registry: AUDIT_REGISTRY,
        [`${AUDIT_REGISTRY.replace(/^https?:/, '')}:_authToken`]: '123',
      },
    })

    expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
    expect(exitCode).toBe(0)
  })

  test('audit endpoint does not exist', async () => {
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(404, {})
    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits')
      .reply(404, {})

    await expect(audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      dev: true,
      fetchRetries: 0,
      ignoreRegistryErrors: false,
      production: false,
    })).rejects.toThrow(AuditEndpointNotExistsError)
  })

  test('audit: CVEs in ignoreCves do not show up', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.ALL_VULN_RESP)

    const { exitCode, output } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'moderate',
      dir: tmp,
      rootProjectManifestDir: tmp,
      rootProjectManifest: {},
      auditConfig: {
        ignoreCves: [
          'CVE-2019-10742',
          'CVE-2020-28168',
          'CVE-2021-3749',
          'CVE-2020-7598',
        ],
      },
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit: ignored vulnerabilities count reflects all ignored paths', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    nock(registries.default)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, {
        actions: [],
        advisories: {
          '999999': {
            findings: [
              {
                version: '1.0.0',
                paths: [
                  'a>pkg',
                  'b>pkg',
                  'c>pkg',
                  'd>pkg',
                  'e>pkg',
                  'f>pkg',
                  'g>pkg',
                ],
                dev: false,
                optional: false,
                bundled: false,
              },
            ],
            id: 999999,
            created: '2026-01-01T00:00:00.000Z',
            updated: '2026-01-01T00:00:00.000Z',
            title: 'Test advisory',
            found_by: { name: 'test' },
            reported_by: { name: 'test' },
            module_name: 'test-package',
            cves: ['CVE-TEST-123'],
            vulnerable_versions: '<2.0.0',
            patched_versions: '>=2.0.0',
            overview: 'test',
            recommendation: 'upgrade',
            references: 'test',
            access: 'public',
            severity: 'high',
            cwe: 'CWE-79',
            github_advisory_id: 'GHSA-test-1234',
            metadata: {
              module_type: 'package',
              exploitability: 1,
              affected_components: 'test',
            },
            url: 'https://example.com',
          },
        },
        muted: [],
        metadata: {
          vulnerabilities: {
            info: 0,
            low: 0,
            moderate: 0,
            high: 7,
            critical: 0,
          },
          dependencies: 1,
          devDependencies: 0,
          optionalDependencies: 0,
          totalDependencies: 1,
        },
      })

    const { output } = await audit.handler({
      auditLevel: 'low',
      dir: tmp,
      rootProjectManifestDir: tmp,
      userConfig: {},
      rawConfig,
      registries,
      rootProjectManifest: {},
      auditConfig: {
        ignoreCves: ['CVE-TEST-123'],
      },
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    })

    expect(stripAnsi(output)).toContain('Severity: 7 high (7 ignored)')
  })

  test('audit: CVEs in ignoreGhsas do not show up', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.ALL_VULN_RESP)

    const { exitCode, output } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'moderate',
      dir: tmp,
      rootProjectManifestDir: tmp,
      rootProjectManifest: {},
      auditConfig: {
        ignoreGhsas: [
          'GHSA-42xw-2xvc-qx8m',
          'GHSA-4w2v-q235-vp99',
          'GHSA-cph5-m8f7-6c5x',
          'GHSA-vh95-rmgr-6w4m',
        ],
      },
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit: CVEs in ignoreCves do not show up when JSON output is used', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    nock(AUDIT_REGISTRY)
      .post('/-/npm/v1/security/audits/quick')
      .reply(200, responses.ALL_VULN_RESP)

    const { exitCode, output } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'moderate',
      dir: tmp,
      rootProjectManifestDir: tmp,
      json: true,
      rootProjectManifest: {},
      auditConfig: {
        ignoreCves: [
          'CVE-2019-10742',
          'CVE-2020-28168',
          'CVE-2021-3749',
          'CVE-2020-7598',
        ],
      },
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })
})
