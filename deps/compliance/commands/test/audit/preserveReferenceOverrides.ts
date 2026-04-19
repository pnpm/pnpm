import path from 'node:path'

import { audit } from '@pnpm/deps.compliance.commands'
import { fixtures } from '@pnpm/test-fixtures'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { readYamlFileSync } from 'read-yaml-file'

import { DEFAULT_OPTS } from './utils/options.js'
import * as responses from './utils/responses/index.js'

const f = fixtures(import.meta.dirname)

const registries = DEFAULT_OPTS.registries

beforeEach(async () => {
  await setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

test('overrides with references (via $) are preserved during audit --fix', async () => {
  const tmp = f.prepare('preserve-reference-overrides')

  getMockAgent().get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/advisories/bulk', method: 'POST' })
    .reply(200, responses.ALL_VULN_RESP)

  const { manifest: initialManifest } = await readProjectManifest(tmp)

  const { exitCode, output } = await audit.handler({
    ...DEFAULT_OPTS,
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

  const manifest = readYamlFileSync<any>(path.join(tmp, 'pnpm-workspace.yaml')) // eslint-disable-line
  expect(manifest.overrides?.['is-positive']).toBe('$is-positive')
})
