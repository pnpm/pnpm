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

test('ignores are added for vulnerable dependencies with no resolutions', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

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
  const cveList = manifest.auditConfig?.ignoreCves
  expect(cveList?.length).toBe(2)
  expect(cveList).toStrictEqual(expect.arrayContaining(['CVE-2017-16115', 'CVE-2017-16024']))
})

test('the specified vulnerabilities are ignored', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: false,
    ignore: ['CVE-2017-16115'],
  })

  expect(exitCode).toBe(0)
  expect(output).toContain('1 new vulnerabilities were ignored')

  const manifest = readYamlFileSync<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.auditConfig?.ignoreCves).toStrictEqual(['CVE-2017-16115'])
})

test('no ignores are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
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

test('ignored CVEs are not duplicated', async () => {
  const tmp = f.prepare('has-vulnerabilities')
  const existingCves = [
    'CVE-2019-10742',
    'CVE-2020-7598',
    'CVE-2017-16115',
    'CVE-2017-16024',
  ]

  getMockAgent().get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreCves: existingCves,
    },
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: false,
    ignoreUnfixable: true,
  })
  expect(exitCode).toBe(0)
  expect(output).toBe('No new vulnerabilities were ignored')

  const manifest = readYamlFileSync<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.auditConfig?.ignoreCves).toStrictEqual(expect.arrayContaining(existingCves))
})
