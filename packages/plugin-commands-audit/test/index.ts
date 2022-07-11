import path from 'path'
import { audit } from '@pnpm/plugin-commands-audit'
import nock from 'nock'
import stripAnsi from 'strip-ansi'
import * as responses from './utils/responses'

const registries = {
  default: 'https://registry.npmjs.org/',
}

test('audit', async () => {
  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.ALL_VULN_RESP)

  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    registries,
  })
  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('audit --dev', async () => {
  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.DEV_VULN_ONLY_RESP)

  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    dev: true,
    production: false,
    registries,
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
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    registries,
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('audit: no vulnerabilities', async () => {
  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.NO_VULN_RESP)

  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, '../../../fixtures/has-outdated-deps'),
    registries,
  })

  expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
  expect(exitCode).toBe(0)
})

test('audit --json', async () => {
  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.ALL_VULN_RESP)

  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    json: true,
    registries,
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
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    dev: true,
    registries,
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
    dir: path.join(__dirname, 'fixtures/has-vulnerabilities'),
    dev: true,
    fetchRetries: 0,
    ignoreRegistryErrors: true,
    production: false,
    registries,
  })

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toBe(`The audit endpoint (at ${registries.default}-/npm/v1/security/audits) responded with 500: {"message":"Something bad happened"}`)
})
