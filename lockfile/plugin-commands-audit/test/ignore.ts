import path from 'path'
import { fixtures } from '@pnpm/test-fixtures'
import { type ProjectManifest } from '@pnpm/types'
import { audit } from '@pnpm/plugin-commands-audit'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import * as responses from './utils/responses'

const f = fixtures(__dirname)
const registries = {
  default: 'https://registry.npmjs.org/',
}
const rawConfig = {
  registry: registries.default,
}

test('ignores are added for vulnerable dependencies with no resolutions', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    fix: false,
    userConfig: {},
    rawConfig,
    registries,
    virtualStoreDirMaxLength: 120,
    ignoreVulnerabilities: '',
  })

  expect(exitCode).toBe(0)
  expect(output).toContain('2 new ignores were added to package.json')

  const manifest = loadJsonFile.sync<ProjectManifest>(path.join(tmp, 'package.json'))
  const cveList = manifest.pnpm?.auditConfig?.ignoreCves
  expect(cveList?.length).toBe(2)
  expect(cveList).toStrictEqual(expect.arrayContaining(['CVE-2017-16115', 'CVE-2017-16024']))
})

test('no ignores are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.NO_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    fix: false,
    userConfig: {},
    rawConfig,
    registries,
    virtualStoreDirMaxLength: 120,
    ignoreVulnerabilities: '',
  })

  expect(exitCode).toBe(0)
  expect(output).toBe('No new ignores were added')
})

test('Ignored CVEs are not duplicated', async () => {
  const tmp = f.prepare('has-vulnerabilities')
  const existingCves = [
    'CVE-2019-10742',
    'CVE-2020-7598',
    'CVE-2017-16115',
    'CVE-2017-16024',
  ]

  {
    const { manifest, writeProjectManifest } = await readProjectManifest(tmp)
    manifest.pnpm = {
      ...manifest.pnpm,
      auditConfig: {
        ignoreCves: existingCves,
      },
    }
    await writeProjectManifest(manifest)
  }

  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    fix: false,
    userConfig: {},
    rawConfig,
    registries,
    virtualStoreDirMaxLength: 120,
    ignoreVulnerabilities: '',
  })
  expect(exitCode).toBe(0)
  expect(output).toBe('No new ignores were added')

  const manifest = loadJsonFile.sync<ProjectManifest>(path.join(tmp, 'package.json'))
  expect(manifest.pnpm?.auditConfig?.ignoreCves).toStrictEqual(expect.arrayContaining(existingCves))
})
