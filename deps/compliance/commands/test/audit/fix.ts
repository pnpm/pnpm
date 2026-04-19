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

test('overrides are added for vulnerable dependencies', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    minimumReleaseAge: 1440,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)
  expect(output).toContain('entries were added to minimumReleaseAgeExclude')

  const manifest = readYamlFileSync<{ overrides?: Record<string, string>, minimumReleaseAgeExclude?: string[] }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('>=0.18.1')
  expect(manifest.overrides?.['sync-exec@>=0.0.0']).toBeFalsy()

  // minimumReleaseAgeExclude should contain the minimum patched versions
  expect(manifest.minimumReleaseAgeExclude).toContain('axios@0.18.1')
  expect(manifest.minimumReleaseAgeExclude).toContain('axios@0.21.1')
  expect(manifest.minimumReleaseAgeExclude).toContain('axios@0.21.2')
  // unfixable advisories (patched_versions: "<0.0.0") should not be included
  expect(manifest.minimumReleaseAgeExclude).not.toContain('sync-exec@0.0.0')
  expect(manifest.minimumReleaseAgeExclude).not.toContain('timespan@0.0.0')
})

test('no overrides are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.NO_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)
  expect(output).toBe('No fixes were made')
})

test('GHSAs in the ignore list are not added as overrides', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        // Denial of Service in axios (<=0.18.0)
        'GHSA-42xw-2xvc-qx8m',
      ],
    },
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })
  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = readYamlFileSync<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBeFalsy()
})
