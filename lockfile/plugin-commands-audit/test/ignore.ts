import path from 'node:path'

import { clearDispatcherCache } from '@pnpm/fetch'
import { audit } from '@pnpm/plugin-commands-audit'
import { fixtures } from '@pnpm/test-fixtures'
import { readYamlFileSync as readYamlFile } from 'read-yaml-file'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

import { DEFAULT_OPTS } from './index.js'
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
const registries = {
  default: 'https://registry.npmjs.org/',
}

beforeEach(() => {
  setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

test('ignores are added for vulnerable dependencies with no resolutions', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent()!.get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...DEFAULT_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: false,
    ignoreUnfixable: true,
  })

  expect(exitCode).toBe(0)
  expect(output).toContain('2 new vulnerabilities were ignored')

  const manifest = readYamlFile<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  const cveList = manifest.auditConfig?.ignoreCves
  expect(cveList?.length).toBe(2)
  expect(cveList).toStrictEqual(expect.arrayContaining(['CVE-2017-16115', 'CVE-2017-16024']))
})

test('the specified vulnerabilities are ignored', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  getMockAgent()!.get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...DEFAULT_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: false,
    ignore: ['CVE-2017-16115'],
  })

  expect(exitCode).toBe(0)
  expect(output).toContain('1 new vulnerabilities were ignored')

  const manifest = readYamlFile<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.auditConfig?.ignoreCves).toStrictEqual(['CVE-2017-16115'])
})

test('no ignores are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  getMockAgent()!.get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
    .reply(200, responses.NO_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...DEFAULT_OPTS,
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

  getMockAgent()!.get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    ...DEFAULT_OPTS,
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

  const manifest = readYamlFile<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.auditConfig?.ignoreCves).toStrictEqual(expect.arrayContaining(existingCves))
})
