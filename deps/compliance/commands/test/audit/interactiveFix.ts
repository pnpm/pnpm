import path from 'node:path'

import { afterEach, beforeEach, expect, jest, test } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import chalk from 'chalk'
import { readYamlFileSync } from 'read-yaml-file'

import { AUDIT_REGISTRY, AUDIT_REGISTRY_OPTS } from './utils/options.js'
import * as responses from './utils/responses/index.js'

jest.unstable_mockModule('@inquirer/prompts', () => {
  class Separator {
    separator: string
    readonly type = 'separator' as const
    constructor (separator: string) {
      this.separator = separator
    }
  }
  return {
    Separator,
    checkbox: jest.fn(),
    confirm: jest.fn(),
    input: jest.fn(),
    password: jest.fn(),
    select: jest.fn(),
  }
})
const { checkbox, Separator } = await import('@inquirer/prompts')
const { audit } = await import('@pnpm/deps.compliance.commands')

const mockCheckbox = jest.mocked(checkbox)

const f = fixtures(import.meta.dirname)

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
  mockCheckbox.mockClear()
})

test('audit --fix -i shows interactive prompt and only fixes selected vulnerabilities', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  mockCheckbox.mockResolvedValue(['xmlhttprequest-ssl@<1.6.1'])

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
  mockCheckbox.mockResolvedValue(['xmlhttprequest-ssl@<1.6.1'])

  await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    interactive: true,
  })

  expect(mockCheckbox).toHaveBeenCalledWith(
    expect.objectContaining({
      message:
        'Choose which vulnerabilities to fix ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)` +
        '\n\nEnter to start fixing. Ctrl-c to cancel.',
      pageSize: process.stdout.rows == null ? 7 : Math.max(7, process.stdout.rows - 6),
    })
  )

  const callArgs = mockCheckbox.mock.calls[0][0]
  expect((callArgs.theme?.style?.highlight as (str: string) => string)?.('focused row')).toBe('focused row')
  const choices = callArgs.choices as Array<{ type?: string; name?: string; value?: string }>

  const separatorNames = choices
    .filter((c) => c instanceof Separator || c.type === 'separator')
    .map((c) => c instanceof Separator ? c.separator : String(c))

  expect(separatorNames.some((s: string) => s.includes('critical'))).toBe(true)
  expect(separatorNames.some((s: string) => s.includes('high'))).toBe(true)
  expect(separatorNames.some((s: string) => s.includes('moderate'))).toBe(true)
})

test('audit --fix -i collapses advisories that share module_name@vulnerable_versions', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  mockCheckbox.mockResolvedValue(['minimatch@<3.1.3'])

  await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    interactive: true,
  })

  const callArgs = mockCheckbox.mock.calls[0][0]
  const choices = callArgs.choices as Array<Record<string, unknown>>
  const valueChoices = choices.filter((c) => 'value' in c)
  const minimatchRows = valueChoices.filter((c) => c.value === 'minimatch@<3.1.3')
  expect(minimatchRows).toHaveLength(1)
  expect(String(minimatchRows[0].name)).toMatch(/GHSA-3ppc-4f35-3m26.*GHSA-7r86-[a-z0-9-]+/)
})

test('audit --fix -i with auditLevel filters before showing prompt', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  mockCheckbox.mockResolvedValue(['xmlhttprequest-ssl@<1.6.1'])

  await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'critical',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    interactive: true,
  })

  const callArgs = mockCheckbox.mock.calls[0][0]
  const choices = callArgs.choices as Array<Record<string, unknown>>
  const separatorNames = choices
    .filter((c) => c instanceof Separator || c.type === 'separator')
    .map((c) => c instanceof Separator ? c.separator : String(c))
  expect(separatorNames.filter((s: string) => s.includes('critical') || s.includes('high') || s.includes('moderate') || s.includes('low'))).toHaveLength(1)
  expect(separatorNames.some((s: string) => s.includes('critical'))).toBe(true)
})
