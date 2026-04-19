import path from 'node:path'

import { jest } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import chalk from 'chalk'
import { readYamlFileSync } from 'read-yaml-file'

import { AUDIT_REGISTRY, AUDIT_REGISTRY_OPTS } from './utils/options.js'
import * as responses from './utils/responses/index.js'

jest.unstable_mockModule('enquirer', () => ({ default: { prompt: jest.fn() } }))
const { default: enquirer } = await import('enquirer')
const { audit } = await import('@pnpm/deps.compliance.commands')

const prompt = jest.mocked(enquirer.prompt)

const f = fixtures(import.meta.dirname)

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
  prompt.mockClear()
})

test('audit --fix -i shows interactive prompt and only fixes selected vulnerabilities', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // Mock the user selecting only the xmlhttprequest-ssl critical advisory
  prompt.mockResolvedValue({
    selectedVulnerabilities: [
      {
        value: 'xmlhttprequest-ssl@<1.6.1',
        name: 'xmlhttprequest-ssl@<1.6.1',
      },
    ],
  })

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    interactive: true,
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = readYamlFileSync<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))

  // Only the selected advisory should be fixed
  expect(manifest.overrides?.['xmlhttprequest-ssl@<1.6.1']).toBe('^1.6.1')

  // Other advisories should NOT be fixed
  expect(manifest.overrides?.['axios@<=0.18.0']).toBeFalsy()
  expect(manifest.overrides?.['nodemailer@<6.4.16']).toBeFalsy()
  expect(manifest.overrides?.['cryptiles@<4.1.2']).toBeFalsy()
})

test('audit --fix -i prompt is called with correct structure', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  // Mock selecting one advisory so the fix proceeds
  prompt.mockResolvedValue({
    selectedVulnerabilities: [
      {
        value: 'xmlhttprequest-ssl@<1.6.1',
        name: 'xmlhttprequest-ssl@<1.6.1',
      },
    ],
  })

  await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    interactive: true,
  })

  expect(prompt).toHaveBeenCalledWith(
    expect.objectContaining({
      footer: '\nEnter to start fixing. Ctrl-c to cancel.',
      message:
        'Choose which vulnerabilities to fix ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
      name: 'selectedVulnerabilities',
      type: 'multiselect',
    })
  )

  // Verify choices are grouped by severity
  const choices = (prompt.mock.calls[0][0] as unknown as Record<string, unknown>).choices as Array<{ name: string }>
  const groupNames = choices.map((g) => g.name)
  // Should have severity groups (order: critical, high, moderate, low)
  expect(groupNames[0]).toBe('[critical]')
  expect(groupNames[1]).toBe('[high]')
  expect(groupNames[2]).toBe('[moderate]')
})

test('audit --fix -i collapses advisories that share module_name@vulnerable_versions', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  prompt.mockResolvedValue({
    selectedVulnerabilities: [
      { value: 'minimatch@<3.1.3', name: 'minimatch@<3.1.3' },
    ],
  })

  await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    interactive: true,
  })

  // The mock fixture has 2 distinct advisories for minimatch@<3.1.3 with
  // different GHSA IDs; they must render as a single interactive choice
  // whose rendered row lists both GHSA IDs.
  const choices = (prompt.mock.calls[0][0] as unknown as Record<string, unknown>).choices as Array<{ choices: Array<{ value: string, message?: string }> }>
  const allRows = choices.flatMap((g) => g.choices)
  const minimatchRows = allRows.filter((c) => c.value === 'minimatch@<3.1.3')
  expect(minimatchRows).toHaveLength(1)
  expect(minimatchRows[0].message).toMatch(/GHSA-3ppc-4f35-3m26.*GHSA-7r86-[a-z0-9-]+/)
})

test('audit --fix -i with auditLevel filters before showing prompt', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  prompt.mockResolvedValue({
    selectedVulnerabilities: [
      {
        value: 'xmlhttprequest-ssl@<1.6.1',
        name: 'xmlhttprequest-ssl@<1.6.1',
      },
    ],
  })

  await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'critical',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    interactive: true,
  })

  // Verify only critical severity group is shown
  const choices = (prompt.mock.calls[0][0] as unknown as Record<string, unknown>).choices as Array<{ name: string }>
  const groupNames = choices.map((g) => g.name)
  expect(groupNames).toStrictEqual(['[critical]'])
})
