import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { AuditEndpointNotExistsError } from '@pnpm/deps.compliance.audit'
import { audit } from '@pnpm/deps.compliance.commands'
import { install } from '@pnpm/installing.commands'
import { fixtures } from '@pnpm/test-fixtures'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

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
  beforeEach(async () => {
    await setupMockAgent()
  })
  afterEach(async () => {
    await teardownMockAgent()
  })
  test('audit', async () => {
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
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
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
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
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
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
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
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
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
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

  test('audit exits 0 when every found vulnerability is below --audit-level', async () => {
    // Only a single moderate advisory against axios. With --audit-level=high
    // the table is empty (so exitCode is 0), but the summary still reports
    // the moderate vulnerability so the user knows it exists.
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, {
        axios: [
          {
            id: 99000001,
            url: 'https://github.com/advisories/GHSA-below-level-test-0001',
            title: 'moderate axios advisory for audit-level test',
            severity: 'moderate',
            vulnerable_versions: '<=0.99.0',
            cwe: [] as string[],
          },
        ],
      })

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'high',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
    })

    expect(exitCode).toBe(0)
    expect(stripAnsi(output)).toBe('1 vulnerabilities found\nSeverity: 1 moderate')
  })

  test('audit --json respects audit-level', async () => {
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { exitCode, output } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'critical',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      json: true,
      dev: true,
    })

    expect(exitCode).toBe(1)
    const parsed = JSON.parse(output)
    // DEV_VULN_ONLY_RESP has 2 critical advisories — only those should be
    // included at audit-level=critical.
    expect(Object.keys(parsed.advisories)).toHaveLength(2)
    for (const advisory of Object.values(parsed.advisories) as Array<{ severity: string }>) {
      expect(advisory.severity).toBe('critical')
    }
  })

  test('audit --json filters advisories by audit-level', async () => {
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
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
    // At audit-level=high, only high/critical advisories should remain.
    for (const advisory of Object.values(parsed.advisories) as Array<{ severity: string }>) {
      expect(['high', 'critical']).toContain(advisory.severity)
    }
    expect(Object.keys(parsed.advisories).length).toBeGreaterThan(0)
  })

  test('audit does not exit with code 1 if the registry responds with a non-200 response and ignoreRegistryErrors is used', async () => {
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(500, { message: 'Something bad happened' })
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
    expect(stripAnsi(output)).toBe(`The audit endpoint (at ${AUDIT_REGISTRY}-/npm/v1/security/advisories/bulk) responded with 500: {"message":"Something bad happened"}`)
  })

  test('audit sends authToken', async () => {
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({
        path: '/-/npm/v1/security/advisories/bulk',
        method: 'POST',
        headers: { authorization: 'Bearer 123' },
      })
      .reply(200, responses.NO_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      configByUri: {
        '//audit.registry/': { creds: { authToken: '123' } },
      },
    })

    expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
    expect(exitCode).toBe(0)
  })

  test('audit endpoint does not exist', async () => {
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
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

  test('audit: advisories in ignoreGhsas do not show up', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
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

  test('audit: advisories in ignoreGhsas do not show up when JSON output is used', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, responses.ALL_VULN_RESP)

    const { exitCode, output } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'moderate',
      dir: tmp,
      rootProjectManifestDir: tmp,
      json: true,
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

  test('audit --audit-level info', async () => {
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, responses.INFO_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      auditLevel: 'info',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toContain('just some info')
    expect(stripAnsi(output)).toContain('info')
  })

  test('audit defaults to low level and ignores info', async () => {
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
      .reply(200, responses.INFO_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
    })

    expect(exitCode).toBe(0)
    expect(stripAnsi(output)).toBe(`1 vulnerabilities found
Severity: 1 info`)
  })
})
