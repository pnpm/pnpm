import { type Dispatcher, getGlobalDispatcher, MockAgent, setGlobalDispatcher } from 'undici'

let originalDispatcher: Dispatcher | null = null
let currentMockAgent: MockAgent | null = null

export async function setupMockAgent (): Promise<MockAgent> {
  if (!originalDispatcher) {
    originalDispatcher = getGlobalDispatcher()
  }
  // Dynamic import to avoid circular tsconfig reference with @pnpm/network.fetch
  const { clearDispatcherCache } = await import('@pnpm/network.fetch')
  clearDispatcherCache()
  currentMockAgent = new MockAgent()
  currentMockAgent.disableNetConnect()
  setGlobalDispatcher(currentMockAgent)
  return currentMockAgent
}

export async function teardownMockAgent (): Promise<void> {
  if (currentMockAgent) {
    await currentMockAgent.close()
    currentMockAgent = null
  }
  if (originalDispatcher) {
    setGlobalDispatcher(originalDispatcher)
    originalDispatcher = null
  }
}

export function getMockAgent (): MockAgent | null {
  return currentMockAgent
}
