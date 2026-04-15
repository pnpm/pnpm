import path from 'node:path'

import { audit } from '@pnpm/deps.compliance.commands'
import { fixtures } from '@pnpm/test-fixtures'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { readYamlFileSync } from 'read-yaml-file'

import { AUDIT_REGISTRY, AUDIT_REGISTRY_OPTS } from './utils/options.js'
import * as responses from './utils/responses/index.js'

const f = fixtures(import.meta.dirname)

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

test('ignores are added for vulnerable dependencies with no resolutions', async () => {
  const tmp = f.prepare('has-vulnerabilities')

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
  const tmp = f.prepare('has-vulnerabilities')

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

  expect(exitCode).toBe(0)
  expect(output).toContain('1 new vulnerabilities were ignored')

  const manifest = readYamlFileSync<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  // Stored canonicalized (GHSA prefix upper, suffix lower) regardless of the
  // user-supplied casing.
  expect(manifest.auditConfig?.ignoreGhsas).toStrictEqual(['GHSA-cph5-m8f7-6c5x'])
})

test('no ignores are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

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
  const tmp = f.prepare('has-vulnerabilities')
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
