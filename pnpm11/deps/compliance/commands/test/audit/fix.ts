import fs, { existsSync as fsExistsSync } from 'node:fs'
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

test('no overrides or minimumReleaseAgeExclude entries are added when the inferred patched version was never published', async () => {
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
  // The packument names no version satisfying the inferred `>=0.18.1` patch:
  // the fix was never released, so there is nothing to fix with.
  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/axios', method: 'GET' })
    .reply(200, {
      name: 'axios',
      time: { '0.18.0': '2020-01-01T00:00:00.000Z' },
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
  expect(output).toBe('No fixes were made')
  expect(fsExistsSync(path.join(tmp, 'pnpm-workspace.yaml'))).toBe(false)
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

test('audit.ignorePrune removes ignored GHSAs that are no longer in the report', async () => {
  const tmp = f.prepare('has-vulnerabilities-with-ignored-ghsas')

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
    },
    auditIgnorePrune: true,
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

  expect(collectedInfos).toContain('Removed 1 unused ignored GHSA: GHSA-xxxx-xxxx-xxxx')
})

test('audit.ignorePrune is disabled by default - no pruning', async () => {
  const tmp = f.prepare('has-vulnerabilities-with-ignored-ghsas')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // Without audit.ignorePrune: true, pruning should NOT run
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

  // When pruning doesn't run, the auditConfig stays unchanged
  const manifest = readYamlFileSync<{ auditConfig?: { ignoreGhsas?: string[] } }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.auditConfig?.ignoreGhsas).toContain('GHSA-xxxx-xxxx-xxxx')
})

// GHSA ids are case-insensitive; lowercase version should match uppercase in report
test('audit.ignorePrune handles case normalization', async () => {
  const tmp = f.prepare('has-vulnerabilities-with-ignored-ghsas')

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
    },
    auditIgnorePrune: true,
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

test('audit.ignorePrune persists the canonical form even when nothing is removed', async () => {
  const tmp = f.prepare('has-vulnerabilities-with-ignored-ghsas')

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
    },
    auditIgnorePrune: true,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)
  expect(collectedInfos.some((message) => message.includes('unused ignored GHSA'))).toBe(false)

  const manifest = readYamlFileSync<{ auditConfig?: { ignoreGhsas?: string[] } }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.auditConfig?.ignoreGhsas).toEqual(['GHSA-42xw-2xvc-qx8m'])
})

test('audit.ignorePrune removes all entries when none are relevant', async () => {
  const tmp = f.prepare('has-vulnerabilities-with-ignored-ghsas')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // Only GHSAs that don't exist in the report - all should be pruned
  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'GHSA-xxxx-0000-0001',
        'GHSA-xxxx-0000-0002',
      ],
    },
    auditIgnorePrune: true,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)

  const manifest = readYamlFileSync<{ auditConfig?: { ignoreGhsas?: string[] } }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.auditConfig?.ignoreGhsas).toBeUndefined()
})

test('audit.ignorePrune edits an inline (flow-style) auditConfig in place', async () => {
  const tmp = f.prepare('has-vulnerabilities-with-ignored-ghsas')
  fs.writeFileSync(
    path.join(tmp, 'pnpm-workspace.yaml'),
    'packages:\n  - \'.\'\nsharedWorkspaceLockfile: false\nauditConfig: { ignoreGhsas: [GHSA-42xw-2xvc-qx8m, GHSA-xxxx-xxxx-xxxx] }\n'
  )

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'GHSA-42xw-2xvc-qx8m',
        'GHSA-xxxx-xxxx-xxxx',
      ],
    },
    auditIgnorePrune: true,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)

  // The retained GHSA is edited in place inside the flow-style block, rather
  // than the whole auditConfig being reformatted into block style.
  const rawContent = fs.readFileSync(path.join(tmp, 'pnpm-workspace.yaml'), 'utf8')
  expect(rawContent).toContain(
    'auditConfig: { ignoreGhsas: [ GHSA-42xw-2xvc-qx8m ] }'
  )

  const manifest = readYamlFileSync<{ auditConfig?: { ignoreGhsas?: string[] } }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.auditConfig?.ignoreGhsas).toEqual(['GHSA-42xw-2xvc-qx8m'])
})

test('audit.ignorePrune updates the canonical audit.ignore list', async () => {
  const tmp = f.prepare('has-vulnerabilities-with-ignored-ghsas')
  fs.writeFileSync(
    path.join(tmp, 'pnpm-workspace.yaml'),
    'packages:\n  - \'.\'\nsharedWorkspaceLockfile: false\naudit:\n  ignorePrune: true\n  ignore:\n    - GHSA-42xw-2xvc-qx8m\n    - GHSA-xxxx-xxxx-xxxx\n'
  )

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // `auditConfig`/`auditIgnorePrune` are the internal fields the config
  // reader derives from the manifest's `audit` section.
  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'GHSA-42xw-2xvc-qx8m',
        'GHSA-xxxx-xxxx-xxxx',
      ],
    },
    auditIgnorePrune: true,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)

  // The retained list must land back on the canonical `audit.ignore` that
  // supplied it — writing the deprecated `auditConfig.ignoreGhsas` instead
  // would let the unchanged canonical list shadow the prune on the next
  // read and restore the stale id.
  const manifest = readYamlFileSync<{ audit?: { ignorePrune?: boolean, ignore?: string[] }, auditConfig?: unknown }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.audit).toStrictEqual({ ignorePrune: true, ignore: ['GHSA-42xw-2xvc-qx8m'] })
  expect(manifest.auditConfig).toBeUndefined()
})

test('audit.ignorePrune sanitizes the removed ids in the log message', async () => {
  const tmp = f.prepare('has-vulnerabilities-with-ignored-ghsas')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // The stale entry carries an ANSI escape from the repository-controlled
  // manifest; the removal message must strip it before the terminal.
  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'GHSA-42xw-2xvc-qx8m',
        'GHSA-xxxx-xxxx-xxxx\u001b[31m',
      ],
    },
    auditIgnorePrune: true,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)
  expect(collectedInfos).toContain('Removed 1 unused ignored GHSA: GHSA-xxxx-xxxx-xxxx[31m')
  expect(collectedInfos.every((message) => !message.includes('\u001b'))).toBe(true)
})

test.each([
  ['the empty string the CLI parser delivers', ''],
  ['a boolean from an rc file', true],
  ['the string form of that boolean', 'true'],
])('a --fix without a method applies the default fix method: %s', async (_label, fix) => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix,
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = readYamlFileSync<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('^0.18.1')
})

test('an invalid --fix value is rejected', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.NO_VULN_RESP)

  await expect(audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: 'bogus',
  })).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_FIX_OPTION' })
})

test('saveExact saves the override as an exact version', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    saveExact: true,
  })

  expect(exitCode).toBe(0)

  const manifest = readYamlFileSync<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('0.18.1')
})

test('savePrefix ~ saves the override as a tilde range', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    savePrefix: '~',
  })

  expect(exitCode).toBe(0)

  const manifest = readYamlFileSync<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('~0.18.1')
})

test('savePrefix = saves the override as an exact = range', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    savePrefix: '=',
  })

  expect(exitCode).toBe(0)

  const manifest = readYamlFileSync<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('=0.18.1')
})

test('savePrefix "" saves the override as an exact version', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    savePrefix: '',
  })

  expect(exitCode).toBe(0)

  const manifest = readYamlFileSync<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('0.18.1')
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

  function publishInfo (time: Record<string, string>, deprecated: string[] = []) {
    return { time, deprecated: new Set(deprecated) }
  }

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
      getPublishTimes: async (pkgName) => publishTimes[pkgName] ? publishInfo(publishTimes[pkgName]) : undefined,
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual(['lodash@4.17.21'])
  })

  test('omits entries for patched versions missing from the packument', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
      advisory('lodash', '<4.17.21', '>=4.17.21'),
    ]
    const publishTimes: Record<string, Record<string, string>> = {
      axios: { '0.18.1': '2026-01-01T00:00:00.000Z' },
      // The packument was fetched but names no 4.17.21: the patched release
      // was never published, so it gets no bypass entry.
      lodash: {},
    }
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async (pkgName) => publishTimes[pkgName] ? publishInfo(publishTimes[pkgName]) : undefined,
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual([])
  })

  test('uses the lowest published version satisfying the patched range', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
    ]
    // 0.18.1 was never published; 0.18.2 is the lowest published version
    // satisfying >=0.18.1 and it is fresh enough to need the bypass.
    const publishTimes: Record<string, Record<string, string>> = {
      axios: { '0.18.2': '2026-01-07T23:30:00.000Z' },
    }
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async (pkgName) => publishTimes[pkgName] ? publishInfo(publishTimes[pkgName]) : undefined,
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual(['axios@0.18.2'])
  })

  test('skips deprecated versions when selecting the lowest published fix', async () => {
    const advisories = [
      advisory('lodash-es', '<4.18.0', '>=4.18.0'),
    ]
    // 4.18.0 is deprecated; 4.18.1 is the lowest non-deprecated published
    // version satisfying >=4.18.0.
    const publishTimes: Record<string, Record<string, string>> = {
      'lodash-es': { '4.18.0': '2026-01-07T23:00:00.000Z', '4.18.1': '2026-01-07T23:30:00.000Z' },
    }
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async (pkgName) => publishTimes[pkgName]
        ? publishInfo(publishTimes[pkgName], ['4.18.0'])
        : undefined,
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual(['lodash-es@4.18.1'])
  })

  test('skips deprecated versions the time map spells differently', async () => {
    const advisories = [
      advisory('lodash-es', '<4.18.0', '>=4.18.0'),
    ]
    // The `time` map spells 4.18.0 with a leading `v` while `versions` — the
    // source of the deprecation set — does not.
    const publishTimes: Record<string, Record<string, string>> = {
      'lodash-es': { 'v4.18.0': '2026-01-07T23:00:00.000Z', '4.18.1': '2026-01-07T23:30:00.000Z' },
    }
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async (pkgName) => publishTimes[pkgName]
        ? publishInfo(publishTimes[pkgName], ['4.18.0'])
        : undefined,
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual(['lodash-es@4.18.1'])
  })

  test('prefers a stable release over a lower-sorting prerelease', async () => {
    const advisories = [
      advisory('axios', '<=1.9.9', '>=1.9.10'),
    ]
    // 2.0.0-beta.1 sorts below 2.0.0 but is not a release users should be
    // pointed at as the fix.
    const publishTimes: Record<string, Record<string, string>> = {
      axios: { '2.0.0-beta.1': '2026-01-07T23:00:00.000Z', '2.0.0': '2026-01-07T23:30:00.000Z' },
    }
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async (pkgName) => publishTimes[pkgName] ? publishInfo(publishTimes[pkgName]) : undefined,
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual(['axios@2.0.0'])
  })

  test('omits entries for patched versions published exactly at the cutoff', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
    ]
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async () => publishInfo({ '0.18.1': '2026-01-07T23:00:00.000Z' }),
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual([])
  })

  test('keeps entries whose publish time is not a valid date string', async () => {
    const advisories = [
      advisory('axios', '<=0.18.0', '>=0.18.1'),
      advisory('lodash', '<4.17.21', '>=4.17.21'),
      advisory('underscore', '<1.13.0', '>=1.13.1'),
    ]
    const publishTimes: Record<string, Record<string, string>> = {
      axios: { '0.18.1': 'not-a-date' },
      // A non-string value smuggled past the registry response type.
      lodash: { '4.17.21': 0 as unknown as string },
      // A bare number parses as epoch 0 with `new Date()`, which is not a
      // real publish timestamp and must be treated as unknown.
      underscore: { '1.13.1': '0' },
    }
    const excludes = await createMinimumReleaseAgeExcludes(advisories, {
      getPublishTimes: async (pkgName) => publishTimes[pkgName] ? publishInfo(publishTimes[pkgName]) : undefined,
      minimumReleaseAge: 60,
      now: new Date('2026-01-08T00:00:00.000Z').getTime(),
    })
    expect(excludes).toEqual(['axios@0.18.1', 'lodash@4.17.21', 'underscore@1.13.1'])
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
