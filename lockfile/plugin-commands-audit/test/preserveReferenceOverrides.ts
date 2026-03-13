import path from 'node:path'

import { clearDispatcherCache } from '@pnpm/fetch'
import { audit } from '@pnpm/plugin-commands-audit'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { fixtures } from '@pnpm/test-fixtures'
import { readYamlFileSync } from 'read-yaml-file'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

import { DEFAULT_OPTS } from './index.js'
import * as responses from './utils/responses/index.js'

const f = fixtures(import.meta.dirname)

const registries = DEFAULT_OPTS.registries

let originalDispatcher: Dispatcher | null = null
let currentMockAgent: MockAgent | null = null

function setupMockAgent (): MockAgent {
  if (!originalDispatcher) {
    originalDispatcher = getGlobalDispatcher()
  }
  clearDispatcherCache()
  currentMockAgent = new MockAgent()
  currentMockAgent.disableNetConnect()
  setGlobalDispatcher(currentMockAgent)
  return currentMockAgent
}

async function teardownMockAgent (): Promise<void> {
  if (currentMockAgent) {
    await currentMockAgent.close()
    currentMockAgent = null
  }
  if (originalDispatcher) {
    setGlobalDispatcher(originalDispatcher)
  }
}

beforeEach(() => {
  setupMockAgent()
})

afterEach(async () => {
  await teardownMockAgent()
})

test('overrides with references (via $) are preserved during audit --fix', async () => {
  const tmp = f.prepare('preserve-reference-overrides')

  currentMockAgent!.get(registries.default.replace(/\/$/, ''))
    .intercept({ path: '/-/npm/v1/security/audits/quick', method: 'POST' })
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
