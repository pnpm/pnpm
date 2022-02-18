import path from 'path'
import { audit } from '@pnpm/plugin-commands-audit'
import nock from 'nock'
import stripAnsi from 'strip-ansi'
import * as responses from '../response-mocks'

test('audit', async () => {
  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })
  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('audit --dev', async () => {
  const registry = 'https://registry.npmjs.org/'
  nock(registry)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.DEV_VULN_ONLY_RESP)

  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    dev: true,
    production: false,
    registries: {
      default: registry,
    },
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('audit --audit-level', async () => {
  const registry = 'https://registry.npmjs.org/'
  nock(registry)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.ALL_VULN_RESP)

  const { output, exitCode } = await audit.handler({
    auditLevel: 'moderate',
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    registries: {
      default: registry,
    },
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('audit: no vulnerabilities', async () => {
  const registry = 'https://registry.npmjs.org/'
  nock(registry)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.NO_VULN_RESP)

  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, '../../../fixtures/has-outdated-deps'),
    registries: {
      default: registry,
    },
  })

  expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
  expect(exitCode).toBe(0)
})

test('audit --json', async () => {
  const registry = 'https://registry.npmjs.org/'
  nock(registry)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.ALL_VULN_RESP)

  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    json: true,
    registries: {
      default: registry,
    },
  })

  const json = JSON.parse(output)
  expect(json.metadata).toBeTruthy()
  expect(exitCode).toBe(1)
})

test.skip('audit does not exit with code 1 if the found vulnerabilities are having lower severity then what we asked for', async () => {
  const registry = 'https://registry.npmjs.org/'
  nock(registry)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.DEV_VULN_ONLY_RESP)

  const { output, exitCode } = await audit.handler({
    auditLevel: 'high',
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    dev: true,
    registries: {
      default: registry,
    },
  })

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toBe(`1 vulnerabilities found
Severity: 1 moderate`)
})

test('audit does not exit with code 1 if the registry responds with a non-200 reponse and ignoreRegistryErrors is used', async () => {
  const registry = 'https://registry-error.com'
  nock(registry)
    .post('/-/npm/v1/security/audits')
    .reply(500, { message: 'Something bad happened' })
  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    dev: true,
    fetchRetries: 0,
    ignoreRegistryErrors: true,
    production: false,
    registries: {
      default: registry,
    },
  })

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toBe('The audit endpoint (at https://registry-error.com/-/npm/v1/security/audits) responded with 500: {"message":"Something bad happened"}')
})
