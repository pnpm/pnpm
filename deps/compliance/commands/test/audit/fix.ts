import path from 'node:path'

import { audit } from '@pnpm/deps.compliance.commands'
import { clearDispatcherCache } from '@pnpm/network.fetch'
import { fixtures } from '@pnpm/test-fixtures'
import { readYamlFileSync } from 'read-yaml-file'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

import { AUDIT_REGISTRY, AUDIT_REGISTRY_OPTS } from './utils/options.js'
import * as responses from './utils/responses/index.js'

let originalDispatcher: Dispatcher | null = null
let currentMockAgent: MockAgent | null = null

function setupMockAgent (): MockAgent {
  if (!originalDispatcher) {
    originalDispatcher = getGlobalDispatcher()
  }
  clearDispatcherCache()
  currentMockAgent = new MockAgent()
  currentMockAgent.disableNetConnect()
  setGlobalDispatcher(currentMockAgent)
  return currentMockAgent
}

async function teardownMockAgent (): Promise<void> {
  if (currentMockAgent) {
    await currentMockAgent.close()
    currentMockAgent = null
  }
  if (originalDispatcher) {
    setGlobalDispatcher(originalDispatcher)
  }
}

function getMockAgent (): MockAgent | null {
  return currentMockAgent
}

const f = fixtures(import.meta.dirname)

beforeEach(() => {
  setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

test('overrides are added for vulnerable dependencies', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent()!.get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = readYamlFileSync<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('>=0.18.1')
  expect(manifest.overrides?.['sync-exec@>=0.0.0']).toBeFalsy()
})

test('no overrides are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  getMockAgent()!.get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
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

test('CVEs found in the allow list are not added as overrides', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent()!.get(AUDIT_REGISTRY.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    auditConfig: {
      ignoreCves: [
        'CVE-2019-10742',
        'CVE-2020-28168',
        'CVE-2021-3749',
        'CVE-2020-7598',
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
  expect(manifest.overrides?.['axios@<0.21.1']).toBeFalsy()
  expect(manifest.overrides?.['minimist@<0.2.1']).toBeFalsy()
  expect(manifest.overrides?.['url-parse@<1.5.6']).toBeTruthy()
})
