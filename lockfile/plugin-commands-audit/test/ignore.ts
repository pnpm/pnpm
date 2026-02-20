import path from 'path'
import { fixtures } from '@pnpm/test-fixtures'
import { audit } from '@pnpm/plugin-commands-audit'
import nock from 'nock'
import { sync as readYamlFile } from 'read-yaml-file'
import * as responses from './utils/responses/index.js'
import { AUDIT_REGISTRY_OPTS, AUDIT_REGISTRY } from './utils/options.js'

const f = fixtures(import.meta.dirname)

test('ignores are added for vulnerable dependencies with no resolutions', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  nock(AUDIT_REGISTRY)
    .post('/-/npm/v1/security/audits/quick')
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

  const manifest = readYamlFile<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  const cveList = manifest.auditConfig?.ignoreCves
  expect(cveList?.length).toBe(2)
  expect(cveList).toStrictEqual(expect.arrayContaining(['CVE-2017-16115', 'CVE-2017-16024']))
})

test('the specified vulnerabilities are ignored', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  nock(AUDIT_REGISTRY)
    .post('/-/npm/v1/security/audits/quick')
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

  const manifest = readYamlFile<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.auditConfig?.ignoreCves).toStrictEqual(['CVE-2017-16115'])
})

test('no ignores are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  nock(AUDIT_REGISTRY)
    .post('/-/npm/v1/security/audits/quick')
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

  nock(AUDIT_REGISTRY)
    .post('/-/npm/v1/security/audits/quick')
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

  const manifest = readYamlFile<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.auditConfig?.ignoreCves).toStrictEqual(expect.arrayContaining(existingCves))
})
