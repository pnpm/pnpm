import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { AuditEndpointNotExistsError } from '@pnpm/audit'
import { clearDispatcherCache } from '@pnpm/fetch'
import { audit } from '@pnpm/plugin-commands-audit'
import { install } from '@pnpm/plugin-commands-installation'
import { fixtures } from '@pnpm/test-fixtures'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

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

const f = fixtures(path.join(import.meta.dirname, 'fixtures'))
const registries = {
  default: 'https://registry.npmjs.org/',
}
const rawConfig = {
  registry: registries.default,
}
export const DEFAULT_OPTS = {
  argv: {
    original: [],
  },
  bail: true,
  bin: 'node_modules/.bin',
  ca: undefined,
  cacheDir: '../cache',
  cert: undefined,
  excludeLinksFromLockfile: false,
  extraEnv: {},
  cliOptions: {},
  fetchRetries: 2,
  fetchRetryFactor: 90,
  fetchRetryMaxtimeout: 90,
  fetchRetryMintimeout: 10,
  filter: [] as string[],
  httpsProxy: undefined,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  key: undefined,
  linkWorkspacePackages: true,
  localAddress: undefined,
  lock: false,
  lockStaleDuration: 90,
  networkConcurrency: 16,
  offline: false,
  pending: false,
  pnpmfile: ['./.pnpmfile.cjs'],
  pnpmHomeDir: '',
  preferWorkspacePackages: true,
  proxy: undefined,
  rawConfig,
  rawLocalConfig: {},
  registries,
  rootProjectManifestDir: '',
  // registry: REGISTRY,
  sort: true,
  storeDir: '../store',
  strictSsl: false,
  userAgent: 'pnpm',
  userConfig: {},
  useRunningStoreServer: false,
  useStoreServer: false,
  workspaceConcurrency: 4,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  peersSuffixMaxLength: 1000,
}

describe('plugin-commands-audit', () => {
  const hasVulnerabilitiesDir = f.find('has-vulnerabilities')
  beforeAll(async () => {
    await install.handler({
      ...DEFAULT_OPTS,
      frozenLockfile: true,
      dir: hasVulnerabilitiesDir,
    })
  })
  beforeEach(() => {
    setupMockAgent()
  })
  afterEach(async () => {
    await teardownMockAgent()
  })
  test('audit', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.ALL_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
    })
    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit --dev', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { output, exitCode } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      dev: true,
      production: false,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit --audit-level', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.ALL_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...DEFAULT_OPTS,
      auditLevel: 'moderate',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit: no vulnerabilities', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.NO_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
    })

    expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
    expect(exitCode).toBe(0)
  })

  test('audit --json', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.ALL_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      json: true,
    })

    const json = JSON.parse(output)
    expect(json.metadata).toBeTruthy()
    expect(exitCode).toBe(1)
  })

  test.skip('audit does not exit with code 1 if the found vulnerabilities are having lower severity then what we asked for', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { output, exitCode } = await audit.handler({
      ...DEFAULT_OPTS,
      auditLevel: 'high',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      dev: true,
    })

    expect(exitCode).toBe(0)
    expect(stripAnsi(output)).toBe(`1 vulnerabilities found
  Severity: 1 moderate`)
  })

  test('audit --json respects audit-level', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      auditLevel: 'critical',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      json: true,
      dev: true,
    })

    expect(exitCode).toBe(0)
    const parsed = JSON.parse(output)
    expect(Object.keys(parsed.advisories)).toHaveLength(0)
  })

  test('audit --json filters advisories by audit-level', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      auditLevel: 'high',
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      json: true,
      dev: true,
    })

    expect(exitCode).toBe(1)
    const parsed = JSON.parse(output)
    // DEV_VULN_ONLY_RESP has 4 high and 2 moderate advisories
    // With audit-level=high, only the 4 high advisories should be included
    expect(Object.keys(parsed.advisories)).toHaveLength(4)
    for (const advisory of Object.values(parsed.advisories) as Array<{ severity: string }>) {
      expect(advisory.severity).toBe('high')
    }
  })

  test('audit does not exit with code 1 if the registry responds with a non-200 response and ignoreRegistryErrors is used', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(500, { message: 'Something bad happened' })
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits', method: 'POST' })
      .reply(500, { message: 'Fallback failed too' })
    const { output, exitCode } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      dev: true,
      fetchRetries: 0,
      ignoreRegistryErrors: true,
      production: false,
    })

    expect(exitCode).toBe(0)
    expect(stripAnsi(output)).toBe(`The audit endpoint (at ${registries.default}-/npm/v1/security/audits/quick) responded with 500: {"message":"Something bad happened"}. Fallback endpoint (at ${registries.default}-/npm/v1/security/audits) responded with 500: {"message":"Fallback failed too"}`)
  })

  test('audit sends authToken', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({
        path: '/-/npm/v1/security/audits/quick',
        method: 'POST',
        headers: { authorization: 'Bearer 123' },
      })
      .reply(200, responses.NO_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      ...DEFAULT_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      rawConfig: {
        registry: registries.default,
        [`${registries.default.replace(/^https?:/, '')}:_authToken`]: '123',
      },
    })

    expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
    expect(exitCode).toBe(0)
  })

  test('audit endpoint does not exist', async () => {
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(404, {})
    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits', method: 'POST' })
      .reply(404, {})

    await expect(audit.handler({
      ...DEFAULT_OPTS,
      dir: hasVulnerabilitiesDir,
      rootProjectManifestDir: hasVulnerabilitiesDir,
      dev: true,
      fetchRetries: 0,
      ignoreRegistryErrors: false,
      production: false,
    })).rejects.toThrow(AuditEndpointNotExistsError)
  })

  test('audit: CVEs in ignoreCves do not show up', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.ALL_VULN_RESP)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      auditLevel: 'moderate',
      dir: tmp,
      rootProjectManifestDir: tmp,
      rootProjectManifest: {},
      auditConfig: {
        ignoreCves: [
          'CVE-2019-10742',
          'CVE-2020-28168',
          'CVE-2021-3749',
          'CVE-2020-7598',
        ],
      },
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit: CVEs in ignoreGhsas do not show up', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.ALL_VULN_RESP)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      auditLevel: 'moderate',
      dir: tmp,
      rootProjectManifestDir: tmp,
      rootProjectManifest: {},
      auditConfig: {
        ignoreGhsas: [
          'GHSA-42xw-2xvc-qx8m',
          'GHSA-4w2v-q235-vp99',
          'GHSA-cph5-m8f7-6c5x',
          'GHSA-vh95-rmgr-6w4m',
        ],
      },
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit: CVEs in ignoreCves do not show up when JSON output is used', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    getMockAgent()!.get(registries.default.replace(/\/$/, ''))
      .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
      .reply(200, responses.ALL_VULN_RESP)

    const { exitCode, output } = await audit.handler({
      ...DEFAULT_OPTS,
      auditLevel: 'moderate',
      dir: tmp,
      rootProjectManifestDir: tmp,
      json: true,
      rootProjectManifest: {},
      auditConfig: {
        ignoreCves: [
          'CVE-2019-10742',
          'CVE-2020-28168',
          'CVE-2021-3749',
          'CVE-2020-7598',
        ],
      },
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })
})
