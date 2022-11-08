import path from 'path'
import { fixtures } from '@pnpm/test-fixtures'
import { ProjectManifest } from '@pnpm/types'
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

test('overrides are added for vulnerable dependencies', async () => {
  const tmp = f.prepare('has-vulnerabilities')

  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.ALL_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    fix: true,
    userConfig: {},
    rawConfig,
    registries,
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = await loadJsonFile<ProjectManifest>(path.join(tmp, 'package.json'))
  expect(manifest.pnpm?.overrides?.['axios@<=0.18.0']).toBe('>=0.18.1')
  expect(manifest.pnpm?.overrides?.['sync-exec@>=0.0.0']).toBeFalsy()
})

test('no overrides are added if no vulnerabilities are found', async () => {
  const tmp = f.prepare('fixture')

  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.NO_VULN_RESP)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    fix: true,
    userConfig: {},
    rawConfig,
    registries,
  })

  expect(exitCode).toBe(0)
  expect(output).toBe('No fixes were made')
})

test('CVEs found in the allow list are not added as overrides', async () => {
  const tmp = f.prepare('has-vulnerabilities')
  {
    const { manifest, writeProjectManifest } = await readProjectManifest(tmp)
    manifest.pnpm = {
      ...manifest.pnpm,
      auditConfig: {
        ignoreCves: [
          'CVE-2019-10742',
          'CVE-2020-28168',
          'CVE-2021-3749',
          'CVE-2020-7598',
        ],
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
    fix: true,
    userConfig: {},
    rawConfig,
    registries,
  })
  expect(exitCode).toBe(0)
  expect(output).toMatch(/Run "pnpm install"/)

  const manifest = await loadJsonFile<ProjectManifest>(path.join(tmp, 'package.json'))
  expect(manifest.pnpm?.overrides?.['axios@<=0.18.0']).toBeFalsy()
  expect(manifest.pnpm?.overrides?.['axios@<0.21.1']).toBeFalsy()
  expect(manifest.pnpm?.overrides?.['minimist@<0.2.1']).toBeFalsy()
  expect(manifest.pnpm?.overrides?.['url-parse@<1.5.6']).toBeTruthy()
})
