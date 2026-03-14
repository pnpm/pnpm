import { addDependenciesToPackage } from '@pnpm/installing.deps-installer'
import { clearDispatcherCache } from '@pnpm/network.fetch'
import { prepareEmpty } from '@pnpm/prepare'
import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

import { testDefaults } from '../utils/index.js'

let originalDispatcher: Dispatcher | null = null
let currentMockAgent: MockAgent | null = null

function setupMockAgent (): MockAgent {
  if (!originalDispatcher) {
    originalDispatcher = getGlobalDispatcher()
  }
  clearDispatcherCache()
  currentMockAgent = new MockAgent()
  currentMockAgent.enableNetConnect()
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

test('blockExoticSubdeps disallows git dependencies in subdependencies', async () => {
  prepareEmpty()

  await expect(addDependenciesToPackage({},
    // @pnpm.e2e/has-aliased-git-dependency has a git-hosted subdependency (say-hi from github:zkochan/hi)
    ['@pnpm.e2e/has-aliased-git-dependency'],
    testDefaults({ blockExoticSubdeps: true, fastUnpack: false })
  )).rejects.toThrow('is not allowed in subdependencies when blockExoticSubdeps is enabled')
})

test('blockExoticSubdeps allows git dependencies in direct dependencies', async () => {
  // Mock the HEAD request that isRepoPublic() in @pnpm/resolving.git-resolver makes to check if the repo is public.
  // Without this, transient network failures cause the resolver to fall back to git+https:// instead of
  // resolving via the codeload tarball URL.
  currentMockAgent!.get('https://github.com')
    .intercept({ path: '/kevva/is-negative', method: 'HEAD' })
    .reply(200)

  const project = prepareEmpty()

  // Direct git dependency should be allowed even when blockExoticSubdeps is enabled
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['kevva/is-negative#1.0.0'],
    testDefaults({ blockExoticSubdeps: true })
  )

  project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({
    'is-negative': 'github:kevva/is-negative#1.0.0',
  })
})

test('blockExoticSubdeps allows registry dependencies in subdependencies', async () => {
  const project = prepareEmpty()

  // A package with only registry subdependencies should work fine
  await addDependenciesToPackage(
    {},
    ['is-positive@1.0.0'],
    testDefaults({ blockExoticSubdeps: true })
  )

  project.has('is-positive')
})

test('blockExoticSubdeps: false (default) allows git dependencies in subdependencies', async () => {
  const project = prepareEmpty()

  // Without blockExoticSubdeps (or with it set to false), git subdeps should be allowed
  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/has-aliased-git-dependency'],
    testDefaults({ blockExoticSubdeps: false, fastUnpack: false })
  )

  const m = project.requireModule('@pnpm.e2e/has-aliased-git-dependency')
  expect(m).toBe('Hi')
})
