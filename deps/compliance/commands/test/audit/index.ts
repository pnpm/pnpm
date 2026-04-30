import crypto from 'node:crypto'
import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { afterEach, beforeAll, beforeEach, describe, expect, test } from '@jest/globals'
import { AuditEndpointNotExistsError } from '@pnpm/deps.compliance.audit'
import { audit } from '@pnpm/deps.compliance.commands'
import { install } from '@pnpm/installing.commands'
import { fixtures } from '@pnpm/test-fixtures'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

import { AUDIT_REGISTRY, AUDIT_REGISTRY_OPTS, DEFAULT_OPTS } from './utils/options.js'
import * as responses from './utils/responses/index.js'

const f = fixtures(path.join(import.meta.dirname, 'fixtures'))
const SCOPED_AUDIT_REGISTRY = 'http://scope.audit.registry/'

describe('plugin-commands-audit', () => {
  const hasVulnerabilitiesDir = f.prepare('has-vulnerabilities')
  const hasSignaturesDir = f.prepare('has-signatures')
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

  test('audit signatures', async () => {
    const key = createSigningKey()
    mockRegistryKey(AUDIT_REGISTRY, key)
    mockRegistryKey(SCOPED_AUDIT_REGISTRY, key)
    getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/signed-pkg', method: 'GET' })
      .reply(200, {
        name: 'signed-pkg',
        time: { '1.0.0': '2023-01-01T00:00:00.000Z' },
        versions: {
          '1.0.0': {
            dist: {
              integrity: 'sha512-test-integrity',
              shasum: 'test-shasum',
              signatures: [{ keyid: key.keyid, sig: key.sign('signed-pkg@1.0.0', 'sha512-test-integrity') }],
              tarball: `${AUDIT_REGISTRY}signed-pkg/-/signed-pkg-1.0.0.tgz`,
            },
            name: 'signed-pkg',
            version: '1.0.0',
          },
        },
      })
    getMockAgent().get(SCOPED_AUDIT_REGISTRY.replace(/\/$/, ''))
      .intercept({ path: '/@scope%2Fsigned-pkg', method: 'GET' })
      .reply(200, {
        name: '@scope/signed-pkg',
        time: { '1.0.0': '2023-01-01T00:00:00.000Z' },
        versions: {
          '1.0.0': {
            dist: {
              integrity: 'sha512-scoped-test-integrity',
              shasum: 'test-shasum',
              signatures: [{ keyid: key.keyid, sig: key.sign('@scope/signed-pkg@1.0.0', 'sha512-scoped-test-integrity') }],
              tarball: `${SCOPED_AUDIT_REGISTRY}@scope/signed-pkg/-/signed-pkg-1.0.0.tgz`,
            },
            name: '@scope/signed-pkg',
            version: '1.0.0',
          },
        },
      })

    const { output, exitCode } = await audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasSignaturesDir,
      registries: { ...AUDIT_REGISTRY_OPTS.registries, '@scope': SCOPED_AUDIT_REGISTRY },
      rootProjectManifestDir: hasSignaturesDir,
    }, ['signatures'])

    expect(exitCode).toBe(0)
    expect(stripAnsi(output)).toContain('audited 2 packages')
    expect(stripAnsi(output)).toContain('2 packages have verified registry signatures')
  })

  test('audit rejects unknown subcommands', async () => {
    await expect(audit.handler({
      ...AUDIT_REGISTRY_OPTS,
      dir: hasSignaturesDir,
      rootProjectManifestDir: hasSignaturesDir,
    }, ['unknown'])).rejects.toMatchObject({ code: 'ERR_PNPM_AUDIT_UNKNOWN_SUBCOMMAND' })
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

function createSigningKey (): {
  keyid: string
  publicKey: string
  sign: (id: string, integrity: string) => string
} {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ec', { namedCurve: 'prime256v1' })
  const publicKeyPem = publicKey.export({ format: 'pem', type: 'spki' }).toString()
  return {
    keyid: 'SHA256:test-key',
    publicKey: publicKeyPem.replace(/-----BEGIN PUBLIC KEY-----|-----END PUBLIC KEY-----|\s/g, ''),
    sign: (id, integrity) => {
      const signer = crypto.createSign('SHA256')
      signer.write(`${id}:${integrity}`)
      signer.end()
      return signer.sign(privateKey, 'base64')
    },
  }
}

function mockRegistryKey (registry: string, key: ReturnType<typeof createSigningKey>): void {
  getMockAgent().get(registry.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/keys', method: 'GET' })
    .reply(200, {
      keys: [{
        expires: null,
        key: key.publicKey,
        keyid: key.keyid,
        keytype: 'ecdsa-sha2-nistp256',
        scheme: 'ecdsa-sha2-nistp256',
      }],
    })
}
