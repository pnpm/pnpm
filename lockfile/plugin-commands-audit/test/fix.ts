import path from 'path'
import { fixtures } from '@pnpm/test-fixtures'
import { audit } from '@pnpm/plugin-commands-audit'
import { sync as readYamlFile } from 'read-yaml-file'
import nock from 'nock'
import * as responses from './utils/responses/index.js'
import { AUDIT_REGISTRY_OPTS, AUDIT_REGISTRY } from './utils/options.js'

const f = fixtures(import.meta.dirname)

test('overrides are added for vulnerable dependencies', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  nock(AUDIT_REGISTRY)
    .post('/-/npm/v1/security/audits/quick')
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

  const manifest = readYamlFile<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBe('>=0.18.1')
  expect(manifest.overrides?.['sync-exec@>=0.0.0']).toBeFalsy()
})

test('no overrides are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  nock(AUDIT_REGISTRY)
    .post('/-/npm/v1/security/audits/quick')
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

  nock(AUDIT_REGISTRY)
    .post('/-/npm/v1/security/audits/quick')
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

  const manifest = readYamlFile<{ overrides?: Record<string, string> }>(path.join(tmp, 'pnpm-workspace.yaml'))
  expect(manifest.overrides?.['axios@<=0.18.0']).toBeFalsy()
  expect(manifest.overrides?.['axios@<0.21.1']).toBeFalsy()
  expect(manifest.overrides?.['minimist@<0.2.1']).toBeFalsy()
  expect(manifest.overrides?.['url-parse@<1.5.6']).toBeTruthy()
})
