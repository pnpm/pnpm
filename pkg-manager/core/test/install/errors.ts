import fs from 'fs'
import { type PnpmError } from '@pnpm/error'
import { clearDispatcherCache } from '@pnpm/fetch'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, mutateModulesInSingleProject } from '@pnpm/core'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { type ProjectRootDir } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'
import { MockAgent, setGlobalDispatcher, getGlobalDispatcher, type Dispatcher } from 'undici'
import { testDefaults } from '../utils/index.js'

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

function getMockAgent (): MockAgent | null {
  return currentMockAgent
}

const f = fixtures(import.meta.dirname)

test('fail if none of the available resolvers support a version spec', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await mutateModulesInSingleProject({
      manifest: {
        dependencies: {
          '@types/plotly.js': '1.44.29',
        },
      },
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults())
    throw new Error('should have failed')
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER')
  expect(err.prefix).toBe(process.cwd())
  expect(err.pkgsStack).toStrictEqual(
    [
      {
        id: '@types/plotly.js@1.44.29',
        name: '@types/plotly.js',
        version: '1.44.29',
      },
    ]
  )
})

test('fail if a package cannot be fetched', async () => {
  prepareEmpty()
  setupMockAgent()
  const mockPool = getMockAgent()!.get(`http://localhost:${REGISTRY_MOCK_PORT}`)
  /* eslint-disable @typescript-eslint/no-explicit-any */
  mockPool.intercept({ path: '/@pnpm.e2e%2Fpkg-with-1-dep', method: 'GET' }) // cspell:disable-line
    .reply(200, loadJsonFileSync<any>(f.find('pkg-with-1-dep.json')))
  mockPool.intercept({ path: '/@pnpm.e2e%2Fdep-of-pkg-with-1-dep', method: 'GET' }) // cspell:disable-line
    .reply(200, loadJsonFileSync<any>(f.find('dep-of-pkg-with-1-dep.json')))
  /* eslint-enable @typescript-eslint/no-explicit-any */
  const tarballContent = fs.readFileSync(f.find('pkg-with-1-dep-100.0.0.tgz'))
  mockPool.intercept({ path: '/@pnpm.e2e/pkg-with-1-dep/-/@pnpm.e2e/pkg-with-1-dep-100.0.0.tgz', method: 'GET' })
    .reply(200, tarballContent, { headers: { 'content-length': String(tarballContent.length) } })
  mockPool.intercept({ path: '/@pnpm.e2e/dep-of-pkg-with-1-dep/-/@pnpm.e2e/dep-of-pkg-with-1-dep-100.1.0.tgz', method: 'GET' })
    .reply(403, 'Forbidden', { headers: { 'content-type': 'text/plain' } })

  let err!: PnpmError
  try {
    await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], testDefaults({}, {}, { retry: { retries: 0 } }))
    throw new Error('should have failed')
  } catch (_err: any) { // eslint-disable-line
    await teardownMockAgent()
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_FETCH_403')
  expect(err.prefix).toBe(process.cwd())
  expect(err.pkgsStack).toStrictEqual(
    [
      {
        id: '@pnpm.e2e/pkg-with-1-dep@100.0.0',
        name: '@pnpm.e2e/pkg-with-1-dep',
        version: '100.0.0',
      },
    ]
  )
})
