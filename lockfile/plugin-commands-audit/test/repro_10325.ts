
import { fixtures } from '@pnpm/test-fixtures'
import { audit } from '@pnpm/plugin-commands-audit'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import nock from 'nock'
import * as responses from './utils/responses/index.js'

const f = fixtures(import.meta.dirname)
const registries = {
  default: 'https://registry.npmjs.org/',
}
const rawConfig = {
  registry: registries.default,
}

test('overrides with references (via $) are preserved during audit --fix', async () => {
  const tmp = f.prepare('repro-10325')

  nock(registries.default)
    .post('/-/npm/v1/security/audits')
    .reply(200, responses.ALL_VULN_RESP)

  const { manifest: initialManifest } = await readProjectManifest(tmp)

  const { exitCode, output } = await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    rootProjectManifest: initialManifest,
    fix: true,
    userConfig: {},
    rawConfig,
    registries,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    overrides: {
      'is-positive': '1.0.0',
    },
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/overrides were added/)

  const { manifest } = await readProjectManifest(tmp)
  expect(manifest.pnpm?.overrides?.['is-positive']).toBe('$is-positive')
})
