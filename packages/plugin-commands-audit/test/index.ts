import path from 'path'
import { audit } from '@pnpm/plugin-commands-audit'
import stripAnsi from 'strip-ansi'

const skipOnNode10 = process.version.split('.')[0] === 'v10' ? test.skip : test

// The audits give different results on Node 10, for some reason
skipOnNode10('audit', async () => {
  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'packages/has-vulnerabilities'),
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })
  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('audit --dev', async () => {
  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'packages/has-vulnerabilities'),
    dev: true,
    production: false,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('audit --audit-level', async () => {
  const { output, exitCode } = await audit.handler({
    auditLevel: 'moderate',
    dir: path.join(__dirname, 'packages/has-vulnerabilities'),
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toMatchSnapshot()
})

test('audit: no vulnerabilities', async () => {
  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, '../../../fixtures/has-outdated-deps'),
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  expect(stripAnsi(output)).toBe('No known vulnerabilities found\n')
  expect(exitCode).toBe(0)
})

test('audit --json', async () => {
  const { output, exitCode } = await audit.handler({
    dir: path.join(__dirname, 'packages/has-vulnerabilities'),
    json: true,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  const json = JSON.parse(output)
  expect(json.metadata).toBeTruthy()
  expect(exitCode).toBe(1)
})

test.skip('audit does not exit with code 1 if the found vulnerabilities are having lower severity then what we asked for', async () => {
  const { output, exitCode } = await audit.handler({
    auditLevel: 'high',
    dir: path.join(__dirname, 'packages/has-vulnerabilities'),
    dev: true,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toBe(`1 vulnerabilities found
Severity: 1 moderate`)
})
