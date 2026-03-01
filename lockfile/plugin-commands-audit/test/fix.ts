import path from 'path'
import { fixtures } from '@pnpm/test-fixtures'
import { audit } from '@pnpm/plugin-commands-audit'
import { sync as readYamlFile } from 'read-yaml-file'
import nock from 'nock'
import * as responses from './utils/responses/index.js'

const f = fixtures(import.meta.dirname)
const registries = {
  default: 'https://registry.npmjs.org/',
}
const rawConfig = {
  registry: registries.default,
}

test('overrides are added for vulnerable dependencies', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  nock(registries.default)
    .post('/-/npm/v1/security/audits/quick')
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    userConfig: {},
    rawConfig,
    registries,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = readYamlFile<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('>=0.18.1')
  expect(manifest.overrides?.['sync-exec@>=0.0.0']).toBeFalsy()
})

test('no overrides are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  nock(registries.default)
    .post('/-/npm/v1/security/audits/quick')
    .reply(200, responses.NO_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    userConfig: {},
    rawConfig,
    registries,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  })

  expect(exitCode).toBe(0)
  expect(output).toBe('No fixes were made')
})

test('CVEs found in the allow list are not added as overrides', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  nock(registries.default)
    .post('/-/npm/v1/security/audits/quick')
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
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
    userConfig: {},
    rawConfig,
    registries,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  })
  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = readYamlFile<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBeFalsy()
  expect(manifest.overrides?.['axios@<0.21.1']).toBeFalsy()
  expect(manifest.overrides?.['minimist@<0.2.1']).toBeFalsy()
  expect(manifest.overrides?.['url-parse@<1.5.6']).toBeTruthy()
})

test('GHSAs found in the allow list are not added as overrides', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  nock(registries.default)
    .post('/-/npm/v1/security/audits/quick')
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    auditConfig: {
      ignoreGhsas: [
        'GHSA-42xw-2xvc-qx8m', // axios CVE-2019-10742
        'GHSA-4w2v-q235-vp99', // axios CVE-2020-28168
        'GHSA-cph5-m8f7-6c5x', // axios CVE-2021-3749
        'GHSA-vh95-rmgr-6w4m', // minimist CVE-2020-7598
      ],
    },
    dir: tmp,
    rootProjectManifestDir: tmp,
    fix: true,
    userConfig: {},
    rawConfig,
    registries,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  })
  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = readYamlFile<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBeFalsy()
  expect(manifest.overrides?.['axios@<0.21.1']).toBeFalsy()
  expect(manifest.overrides?.['axios@<=0.21.1']).toBeFalsy()
  expect(manifest.overrides?.['minimist@<0.2.1']).toBeFalsy()
  expect(manifest.overrides?.['url-parse@<1.5.6']).toBeTruthy()
})
