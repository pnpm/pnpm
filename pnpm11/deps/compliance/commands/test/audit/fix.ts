import fs from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'
import type { AuditAdvisory } from '@pnpm/deps.compliance.audit'
import { audit } from '@pnpm/deps.compliance.commands'
import { type LogBase, streamParser } from '@pnpm/logger'
import { fixtures } from '@pnpm/test-fixtures'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { readYamlFileSync } from 'read-yaml-file'

import { caretRangeForPatched, createMinimumReleaseAgeExcludes } from '../../src/audit/fix.js'
import { AUDIT_REGISTRY, AUDIT_REGISTRY_OPTS } from './utils/options.js'
import * as responses from './utils/responses/index.js'

const f = fixtures(import.meta.dirname)

const collectedInfos: string[] = []

beforeEach(async () => {
  collectedInfos.length = 0
  streamParser.on('data', collectInfos as (msg: LogBase) => void)
  await setupMockAgent()
})

afterEach(async () => {
  streamParser.removeListener('data', collectInfos as (msg: LogBase) => void)
  await teardownMockAgent()
})

function collectInfos (msg: LogBase & { message?: string }): void {
  if (msg.level === 'info' && typeof msg.message === 'string') {
    collectedInfos.push(msg.message)
  }
}

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
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('^0.18.1')
  expect(manifest.overrides?.['sync-exec@>=0.0.0']).toBeFalsy()

  // minimumReleaseAgeExclude should combine versions per module
  const axiosExclude = manifest.minimumReleaseAgeExclude?.find((e) => e.startsWith('axios@'))
  expect(axiosExclude).toBeDefined()
  expect(axiosExclude).toContain('0.18.1')
  expect(axiosExclude).toContain('0.21.1')
  expect(axiosExclude).toContain('0.21.2')
})

test('no minimumReleaseAgeExclude entries are added for patched versions published before the cutoff', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, {
      axios: [
        {
          id: 1,
          title: 'vulnerability in axios',
          severity: 'high',
          vulnerable_versions: '<=0.18.0',
          url: 'https://github.com/advisories/GHSA-mock-mock-mock',
        },
      ],
    })
  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/axios', method: 'GET' })
    .reply(200, {
      name: 'axios',
      time: { '0.18.1': '2020-01-01T00:00:00.000Z' },
    })

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    minimumReleaseAge: 1440,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)
  expect(output).not.toContain('minimumReleaseAgeExclude')

  const manifest = readYamlFileSync<{ overrides?: Record<string, string>, minimumReleaseAgeExclude?: string[] }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('^0.18.1')
  expect(manifest.minimumReleaseAgeExclude).toBeUndefined()
})

test('minimumReleaseAgeExclude entries are added for patched versions published after the cutoff', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, {
      axios: [
        {
          id: 1,
          title: 'vulnerability in axios',
          severity: 'high',
          vulnerable_versions: '<=0.18.0',
          url: 'https://github.com/advisories/GHSA-mock-mock-mock',
        },
      ],
    })
  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/axios', method: 'GET' })
    .reply(200, {
      name: 'axios',
      time: { '0.18.1': new Date().toISOString() },
    })

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    minimumReleaseAge: 1440,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)
  expect(output).toContain('entries were added to minimumReleaseAgeExclude')

  const manifest = readYamlFileSync<{ overrides?: Record<string, string>, minimumReleaseAgeExclude?: string[] }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('^0.18.1')
  expect(manifest.minimumReleaseAgeExclude).toEqual(['axios@0.18.1'])
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

test('audit --fix respects auditLevel and only fixes matching severities', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'critical',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = readYamlFileSync<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))

  // Critical advisories should be fixed
  expect(manifest.overrides?.['xmlhttprequest-ssl@<1.6.1']).toBe('^1.6.1')
  expect(manifest.overrides?.['nodemailer@<6.4.16']).toBe('^6.4.16')
  expect(manifest.overrides?.['netmask@<1.1.0']).toBe('^1.1.0')

  // Non-critical advisories (high, moderate, low) should NOT be fixed
  expect(manifest.overrides?.['axios@<=0.18.0']).toBeFalsy()
  expect(manifest.overrides?.['axios@<0.21.2']).toBeFalsy()
  expect(manifest.overrides?.['url-parse@<1.5.6']).toBeFalsy()
})

test('cleanupUnusedIgnoredGhsas removes GHSAs that are no longer in the report', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // GHSA-42xw-2xvc-qx8m exists in the report (axios <=0.18.0)
  // GHSA-xxxx-xxxx-xxxx does NOT exist in the report - should be removed
  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'GHSA-42xw-2xvc-qx8m',
        'GHSA-xxxx-xxxx-xxxx',
      ],
      cleanupUnusedIgnoredGhsas: true,
    },
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)

  const manifest = readYamlFileSync<{ auditConfig?: { ignoreGhsas?: string[] } }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.auditConfig?.ignoreGhsas).toContain('GHSA-42xw-2xvc-qx8m')
  expect(manifest.auditConfig?.ignoreGhsas).not.toContain('GHSA-xxxx-xxxx-xxxx')

  // The preceding comment and the trailing same-line comment attached to
  // the removed entry must both go with it.
  const rawContent = fs.readFileSync(path.join(tmp, 'pnpm-workspace.yaml'), 'utf8')
  expect(rawContent).not.toContain('Expired GHSA')
  expect(rawContent).not.toContain('trailing comment')

  expect(collectedInfos).toContain('Removed 1 unused ignored GHSA(s): GHSA-xxxx-xxxx-xxxx')
})

test('cleanupUnusedIgnoredGhsas is disabled by default - no cleanup', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // Without cleanupUnusedIgnoredGhsas: true, cleanup should NOT run
  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'GHSA-42xw-2xvc-qx8m',
        'GHSA-xxxx-xxxx-xxxx',
      ],
    },
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)

  // When cleanup doesn't run, the auditConfig stays unchanged
  const manifest = readYamlFileSync<{ auditConfig?: { ignoreGhsas?: string[] } }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.auditConfig?.ignoreGhsas).toContain('GHSA-xxxx-xxxx-xxxx')
})

// GHSA ids are case-insensitive; lowercase version should match uppercase in report
test('cleanupUnusedIgnoredGhsas handles case normalization', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'ghsa-42xw-2xvc-qx8m', // lowercase, should be retained
        'GHSA-XXXX-XXXX-XXXX', // uppercase, NOT in report - should be removed
      ],
      cleanupUnusedIgnoredGhsas: true,
    },
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)

  const manifest = readYamlFileSync<{ auditConfig?: { ignoreGhsas?: string[] } }>(path.join(tmp, 'pnpm-workspace.yaml'))
  // Retained entries are written in their canonical form regardless of the
  // casing the user originally ignored them with.
  expect(manifest.auditConfig?.ignoreGhsas).toEqual(['GHSA-42xw-2xvc-qx8m'])
})

test('cleanupUnusedIgnoredGhsas persists the canonical form even when nothing is removed', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // Both entries match the same advisory (a differently-cased duplicate) —
  // nothing gets removed, but the stored list should still collapse to the
  // single canonical entry.
  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'ghsa-42xw-2xvc-qx8m',
        'GHSA-42XW-2XVC-QX8M',
      ],
      cleanupUnusedIgnoredGhsas: true,
    },
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)
  expect(collectedInfos.some((message) => message.includes('unused ignored GHSA'))).toBe(false)

  const manifest = readYamlFileSync<{ auditConfig?: { ignoreGhsas?: string[] } }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.auditConfig?.ignoreGhsas).toEqual(['GHSA-42xw-2xvc-qx8m'])
})

test('cleanupUnusedIgnoredGhsas cleans up all when none are relevant', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // Only GHSAs that don't exist in the report - all should be cleaned up
  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'GHSA-xxxx-0000-0001',
        'GHSA-xxxx-0000-0002',
      ],
      cleanupUnusedIgnoredGhsas: true,
    },
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)

  const manifest = readYamlFileSync<{ auditConfig?: { ignoreGhsas?: string[] } }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.auditConfig?.ignoreGhsas).toBeUndefined()
})

function advisory (moduleName: string, vulnerableVersions: string, patchedVersions?: string): AuditAdvisory {
  return {
    findings: [],
    id: 0,
    title: '',
    module_name: moduleName,
    vulnerable_versions: vulnerableVersions,
    patched_versions: patchedVersions,
    severity: 'high',
    cwe: '',
    github_advisory_id: '',
    url: '',
  }
}

describe('createMinimumReleaseAgeExcludes', () => {
  // The publish times are unknown: every entry is kept.
  const unknownPublishTimes = async (): Promise<undefined> => undefined

  test('combines multiple advisories for the same module into a single sorted entry', async () => {
    const advisories = [
      advisory('axios', '<0.21.2', '>=0.21.2'),
      advisory('axios', '<=0.18.0', '>=0.18.1'),
      advisory('axios', '<0.21.1', '>=0.21.1'),
    ]
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: unknownPublishTimes,
      minimumReleaseAge: 1440,
    })
    expect(excludes).toEqual(['axios@0.18.1 || 0.21.1 || 0.21.2'])
  })

  test('keeps different modules as separate entries', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
      advisory('lodash', '<4.17.21', '>=4.17.21'),
    ]
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: unknownPublishTimes,
      minimumReleaseAge: 1440,
    })
    expect(excludes).toEqual([
      'axios@0.18.1',
      'lodash@4.17.21',
    ])
  })

  test('skips advisories without patched_versions', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
      advisory('sync-exec', '>=0.0.0'),
    ]
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: unknownPublishTimes,
      minimumReleaseAge: 1440,
    })
    expect(excludes).toEqual(['axios@0.18.1'])
  })

  test('returns empty array when no advisories are fixable', async () => {
    const advisories = [
      advisory('sync-exec', '>=0.0.0'),
    ]
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: unknownPublishTimes,
      minimumReleaseAge: 1440,
    })
    expect(excludes).toEqual([])
  })

  test('deduplicates the same minimum patched version for a module', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
      advisory('axios', '<=0.17.0', '>=0.18.1'),
    ]
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: unknownPublishTimes,
      minimumReleaseAge: 1440,
    })
    expect(excludes).toEqual(['axios@0.18.1'])
  })

  test('omits entries for patched versions published at or before the cutoff', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
      advisory('lodash', '<4.17.21', '>=4.17.21'),
    ]
    const publishTimes: Record<string, Record<string, string>> = {
      axios: { '0.18.1': '2026-01-01T00:00:00.000Z' },
      lodash: { '4.17.21': '2026-01-07T23:30:00.000Z' },
    }
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async (pkgName) => publishTimes[pkgName],
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual(['lodash@4.17.21'])
  })

  test('keeps entries whose publish time is missing from the packument', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
      advisory('lodash', '<4.17.21', '>=4.17.21'),
    ]
    const publishTimes: Record<string, Record<string, string>> = {
      axios: { '0.18.1': '2026-01-01T00:00:00.000Z' },
      lodash: {},
    }
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async (pkgName) => publishTimes[pkgName],
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual(['lodash@4.17.21'])
  })

  test('omits entries for patched versions published exactly at the cutoff', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
    ]
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async () => ({ '0.18.1': '2026-01-07T23:00:00.000Z' }),
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual([])
  })

  test('keeps entries whose publish time is not a valid date string', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
      advisory('lodash', '<4.17.21', '>=4.17.21'),
    ]
    const publishTimes: Record<string, Record<string, string>> = {
      axios: { '0.18.1': 'not-a-date' },
      // A non-string value smuggled past the registry response type.
      lodash: { '4.17.21': 0 as unknown as string },
    }
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async (pkgName) => publishTimes[pkgName],
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual(['axios@0.18.1', 'lodash@4.17.21'])
  })
})

describe('caretRangeForPatched', () => {
  test('converts a >= range to a caret range', () => {
    expect(caretRangeForPatched('>=0.18.1')).toBe('^0.18.1')
  })

  test('picks the minimum version from a complex range', () => {
    expect(caretRangeForPatched('>=1.0.0 <2.0.0')).toBe('^1.0.0')
  })
})
