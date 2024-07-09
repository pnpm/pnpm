import path from 'path'
import { fixtures } from '@pnpm/test-fixtures'
import { audit } from '@pnpm/plugin-commands-audit'
import { install } from '@pnpm/plugin-commands-installation'
import { AuditEndpointNotExistsError } from '@pnpm/audit'
import nock from 'nock'
import stripAnsi from 'strip-ansi'
import * as responses from './utils/responses'

const f = fixtures(path.join(__dirname, 'fixtures'))
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
  pnpmfile: './.pnpmfile.cjs',
  pnpmHomeDir: '',
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
  virtualStoreDirMaxLength: 120,
  peersSuffixMaxLength: 1000,
}

describe('plugin-commands-audit', () => {
  beforeAll(async () => {
    await install.handler({
      ...DEFAULT_OPTS,
      frozenLockfile: true,
      dir: f.find('has-vulnerabilities'),
    })
  })
  test('audit', async () => {
    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(200, responses.ALL_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      dir: f.find('has-vulnerabilities'),
      userConfig: {},
      rawConfig,
      registries,
      virtualStoreDirMaxLength: 120,
    })
    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit --dev', async () => {
    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { output, exitCode } = await audit.handler({
      dir: f.find('has-vulnerabilities'),
      dev: true,
      production: false,
      userConfig: {},
      rawConfig,
      registries,
      virtualStoreDirMaxLength: 120,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit --audit-level', async () => {
    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(200, responses.ALL_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      auditLevel: 'moderate',
      dir: f.find('has-vulnerabilities'),
      userConfig: {},
      rawConfig,
      registries,
      virtualStoreDirMaxLength: 120,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit: no vulnerabilities', async () => {
    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(200, responses.NO_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      dir: f.find('has-outdated-deps'),
      userConfig: {},
      rawConfig,
      registries,
      virtualStoreDirMaxLength: 120,
    })

    expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
    expect(exitCode).toBe(0)
  })

  test('audit --json', async () => {
    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(200, responses.ALL_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      dir: f.find('has-vulnerabilities'),
      json: true,
      userConfig: {},
      rawConfig,
      registries,
      virtualStoreDirMaxLength: 120,
    })

    const json = JSON.parse(output)
    expect(json.metadata).toBeTruthy()
    expect(exitCode).toBe(1)
  })

  test.skip('audit does not exit with code 1 if the found vulnerabilities are having lower severity then what we asked for', async () => {
    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(200, responses.DEV_VULN_ONLY_RESP)

    const { output, exitCode } = await audit.handler({
      auditLevel: 'high',
      dir: f.find('has-vulnerabilities'),
      userConfig: {},
      rawConfig,
      dev: true,
      registries,
      virtualStoreDirMaxLength: 120,
    })

    expect(exitCode).toBe(0)
    expect(stripAnsi(output)).toBe(`1 vulnerabilities found
  Severity: 1 moderate`)
  })

  test('audit does not exit with code 1 if the registry responds with a non-200 response and ignoreRegistryErrors is used', async () => {
    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(500, { message: 'Something bad happened' })
    const { output, exitCode } = await audit.handler({
      dir: f.find('has-vulnerabilities'),
      dev: true,
      fetchRetries: 0,
      ignoreRegistryErrors: true,
      production: false,
      userConfig: {},
      rawConfig,
      registries,
      virtualStoreDirMaxLength: 120,
    })

    expect(exitCode).toBe(0)
    expect(stripAnsi(output)).toBe(`The audit endpoint (at ${registries.default}-/npm/v1/security/audits) responded with 500: {"message":"Something bad happened"}`)
  })

  test('audit sends authToken', async () => {
    nock(registries.default, {
      reqheaders: { authorization: 'Bearer 123' },
    })
      .post('/-/npm/v1/security/audits')
      .reply(200, responses.NO_VULN_RESP)

    const { output, exitCode } = await audit.handler({
      dir: f.find('has-outdated-deps'),
      userConfig: {},
      rawConfig: {
        registry: registries.default,
        [`${registries.default.replace(/^https?:/, '')}:_authToken`]: '123',
      },
      registries,
      virtualStoreDirMaxLength: 120,
    })

    expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
    expect(exitCode).toBe(0)
  })

  test('audit endpoint does not exist', async () => {
    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(404, {})

    await expect(audit.handler({
      dir: f.find('has-vulnerabilities'),
      dev: true,
      fetchRetries: 0,
      ignoreRegistryErrors: false,
      production: false,
      userConfig: {},
      rawConfig,
      registries,
      virtualStoreDirMaxLength: 120,
    })).rejects.toThrow(AuditEndpointNotExistsError)
  })

  test('audit: CVEs in ignoreCves do not show up', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(200, responses.ALL_VULN_RESP)

    const { exitCode, output } = await audit.handler({
      auditLevel: 'moderate',
      dir: tmp,
      userConfig: {},
      rawConfig,
      registries,
      rootProjectManifest: {
        pnpm: {
          auditConfig: {
            ignoreCves: [
              'CVE-2019-10742',
              'CVE-2020-28168',
              'CVE-2021-3749',
              'CVE-2020-7598',
            ],
          },
        },
      },
      virtualStoreDirMaxLength: 120,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })

  test('audit: CVEs in ignoreCves do not show up when JSON output is used', async () => {
    const tmp = f.prepare('has-vulnerabilities')

    nock(registries.default)
      .post('/-/npm/v1/security/audits')
      .reply(200, responses.ALL_VULN_RESP)

    const { exitCode, output } = await audit.handler({
      auditLevel: 'moderate',
      dir: tmp,
      json: true,
      userConfig: {},
      rawConfig,
      registries,
      rootProjectManifest: {
        pnpm: {
          auditConfig: {
            ignoreCves: [
              'CVE-2019-10742',
              'CVE-2020-28168',
              'CVE-2021-3749',
              'CVE-2020-7598',
            ],
          },
        },
      },
      virtualStoreDirMaxLength: 120,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output)).toMatchSnapshot()
  })
})
