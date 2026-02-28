import path from 'path'
import { audit } from '@pnpm/plugin-commands-audit'
import { fixtures } from '@pnpm/test-fixtures'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { sync as readYamlFile } from 'read-yaml-file'
import nock from 'nock'
import * as responses from './utils/responses/index.js'
import { AUDIT_REGISTRY_OPTS, AUDIT_REGISTRY } from './utils/options.js'

const f = fixtures(import.meta.dirname)

test('overrides with references (via $) are preserved during audit --fix', async () => {
  const tmp = f.prepare('preserve-reference-overrides')

  nock(AUDIT_REGISTRY)
    .post('/-/npm/v1/security/audits/quick')
    .reply(200, responses.ALL_VULN_RESP)

  const { manifest: initialManifest } = await readProjectManifest(tmp)

  const { exitCode, output } = await audit.handler({
    ...AUDIT_REGISTRY_OPTS,
    auditLevel: 'moderate',
    dir: tmp,
    rootProjectManifestDir: tmp,
    rootProjectManifest: initialManifest,
    fix: true,
    overrides: {
      'is-positive': '1.0.0',
    },
  })

  expect(exitCode).toBe(0)
  expect(output).toMatch(/overrides were added/)

  const manifest = readYamlFile<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.overrides?.['is-positive']).toBe('$is-positive')
})
