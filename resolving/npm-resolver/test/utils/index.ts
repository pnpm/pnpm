import { readMsgpackFile } from '@pnpm/fs.msgpack-file'
import { clearDispatcherCache } from '@pnpm/fetch'
import { MockAgent, setGlobalDispatcher, getGlobalDispatcher, type Dispatcher } from 'undici'

let originalDispatcher: Dispatcher | null = null
let currentMockAgent: MockAgent | null = null

export function setupMockAgent (): MockAgent {
  if (!originalDispatcher) {
    originalDispatcher = getGlobalDispatcher()
  }
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
  }
}

export function getMockAgent (): MockAgent | null {
  return currentMockAgent
}

export async function retryLoadMsgpackFile<T> (filePath: string): Promise<T> {
  let retry = 0
  /* eslint-disable no-await-in-loop */
  while (true) {
    await delay(500)
    try {
      return await readMsgpackFile<T>(filePath)
    } catch (err: any) { // eslint-disable-line
      if (retry > 2) throw err
      retry++
    }
  }
  /* eslint-enable no-await-in-loop */
}

export async function delay (time: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(() => {
    resolve()
  }, time))
}
