import fs from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { audit } from '@pnpm/deps.compliance.commands'
import { fixtures } from '@pnpm/test-fixtures'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { readYamlFileSync } from 'read-yaml-file'

import { AUDIT_REGISTRY, AUDIT_REGISTRY_OPTS } from './utils/options.js'
import * as responses from './utils/responses/index.js'

const testFixtures = fixtures(import.meta.dirname)

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

// Advisories whose vulnerable_versions can't be inferred into a patched
// range (`>=0.0.0` / `*` cover the entire version space). With no inferable
// fix, these surface as "no resolution" for --ignore-unfixable.
const UNFIXABLE_RESPONSE = {
  axios: [
    {
      id: 90000001,
      url: 'https://github.com/advisories/GHSA-unfixable-test-0001',
      title: 'unfixable axios advisory used for tests',
      severity: 'high',
      vulnerable_versions: '>=0.0.0',
      cwe: [] as string[],
    },
    {
      id: 90000002,
      url: 'https://github.com/advisories/GHSA-unfixable-test-0002',
      title: 'another unfixable axios advisory used for tests',
      severity: 'moderate',
      vulnerable_versions: '*',
      cwe: [] as string[],
    },
  ],
}

// One advisory with an inferable fix and one without.
const MIXED_RESPONSE = {
  axios: [
    {
      id: 90000003,
      url: 'https://github.com/advisories/GHSA-fixable-test-0003',
      title: 'fixable axios advisory used for tests',
      severity: 'low',
      vulnerable_versions: '<0.21.1',
      cwe: [] as string[],
    },
    UNFIXABLE_RESPONSE.axios[0],
  ],
}

test('--ignore exits with 1 when a vulnerability that was not ignored remains', async () => {
  expect(await auditWithIgnores(MIXED_RESPONSE, { ignore: ['GHSA-unfixable-test-0001'] })).toBe(1)
})

test('--ignore exits with 0 when every vulnerability is ignored', async () => {
  expect(await auditWithIgnores(MIXED_RESPONSE, { ignore: ['GHSA-unfixable-test-0001', 'GHSA-fixable-test-0003'] })).toBe(0)
})

test('--ignore exits with 0 when the remaining vulnerabilities were ignored before', async () => {
  expect(await auditWithIgnores(MIXED_RESPONSE, {
    auditConfig: { ignoreGhsas: ['GHSA-fixable-test-0003'] },
    ignore: ['GHSA-unfixable-test-0001'],
  })).toBe(0)
})

test('--ignore-unfixable exits with 1 when a fixable vulnerability remains', async () => {
  expect(await auditWithIgnores(MIXED_RESPONSE, { ignoreUnfixable: true })).toBe(1)
})

test('--ignore-unfixable exits with 0 when the remaining vulnerabilities are below the audit level', async () => {
  expect(await auditWithIgnores(MIXED_RESPONSE, { auditLevel: 'moderate', ignoreUnfixable: true })).toBe(0)
})

test('ignores are added for vulnerable dependencies with no resolutions', async () => {
  const tmp = testFixtures.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, UNFIXABLE_RESPONSE)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: false,
    ignoreUnfixable: true,
  })

  expect(exitCode).toBe(0)
  expect(output).toContain('2 new vulnerabilities were ignored')

  const manifest = readYamlFileSync<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  const ghsaList = manifest.auditConfig?.ignoreGhsas
  expect(ghsaList?.length).toBe(2)
  expect(ghsaList).toStrictEqual(expect.arrayContaining(['GHSA-unfixable-test-0001', 'GHSA-unfixable-test-0002']))
})

test('the specified vulnerabilities are ignored', async () => {
  const tmp = testFixtures.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: false,
    ignore: ['GHSA-cph5-m8f7-6c5x'],
  })

  expect(exitCode).toBe(1)
  expect(output).toContain('1 new vulnerabilities were ignored')

  const manifest = readYamlFileSync<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  // Stored canonicalized (GHSA prefix upper, suffix lower) regardless of the
  // user-supplied casing.
  expect(manifest.auditConfig?.ignoreGhsas).toStrictEqual(['GHSA-cph5-m8f7-6c5x'])
})

test('no ignores are added if no vulnerabilities are found', async () => {
  const tmp = testFixtures.prepare('fixture')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.NO_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: false,
    ignoreUnfixable: true,
  })

  expect(exitCode).toBe(0)
  expect(output).toBe('No new vulnerabilities were ignored')
})

test('ignored GHSAs are not duplicated', async () => {
  const tmp = testFixtures.prepare('has-vulnerabilities')
  const existingGhsas = [
    'GHSA-unfixable-test-0001',
    'GHSA-unfixable-test-0002',
  ]

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, UNFIXABLE_RESPONSE)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: existingGhsas,
    },
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: false,
    ignoreUnfixable: true,
  })
  expect(exitCode).toBe(0)
  expect(output).toBe('No new vulnerabilities were ignored')

  const manifest = readYamlFileSync<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.auditConfig?.ignoreGhsas).toStrictEqual(expect.arrayContaining(existingGhsas))
})

test.each([
  { flags: { fix: true, ignore: ['GHSA-cph5-m8f7-6c5x'] }, message: '"--fix" may not be used with "--ignore"' },
  { flags: { fix: 'update', ignore: ['GHSA-cph5-m8f7-6c5x'] }, message: '"--fix" may not be used with "--ignore"' },
  { flags: { fix: true, ignoreUnfixable: true }, message: '"--fix" may not be used with "--ignore-unfixable"' },
  { flags: { interactive: true, ignoreUnfixable: true }, message: '"--interactive" may not be used with "--ignore-unfixable"' },
])('$message', async ({ flags, message }) => {
  const tmp = testFixtures.prepare('has-vulnerabilities')
  const workspaceManifestPath = path.join(tmp, 'pnpm-workspace.yaml')
  const workspaceManifest = 'packages:\n  - .\n'
  fs.writeFileSync(workspaceManifestPath, workspaceManifest)
  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, MIXED_RESPONSE)

  await expect(audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    dir: tmp,
    rootProjectManifestDir: tmp,
    ...flags,
  })).rejects.toMatchObject({ code: 'ERR_PNPM_AUDIT_FIX_WITH_IGNORE', message })

  expect(getMockAgent().pendingInterceptors()).toMatchObject([{ timesInvoked: 0 }])
  expect(fs.readFileSync(workspaceManifestPath, 'utf8')).toBe(workspaceManifest)
})

async function auditWithIgnores (response: Record<string, unknown>, opts: Partial<audit.AuditOptions>): Promise<number> {
  const tmp = testFixtures.prepare('has-vulnerabilities')
  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, response)
  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'low',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: false,
    ...opts,
  })
  return exitCode
}
