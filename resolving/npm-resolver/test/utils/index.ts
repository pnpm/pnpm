import { MetadataCache, type MetadataType } from '@pnpm/cache.metadata'

export { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

export async function retryLoadFromCache<T> (cacheDir: string, name: string, type: MetadataType): Promise<T> {
  let retry = 0
  /* eslint-disable no-await-in-loop */
  while (true) {
    await delay(500)
    const db = new MetadataCache(cacheDir)
    try {
      const row = db.get(name, type)
      if (row) {
        return JSON.parse(row.data) as T
      }
      if (retry > 2) throw new Error(`No cache entry found for ${name} (${type})`)
      retry++
    } finally {
      db.close()
    }
  }
  /* eslint-enable no-await-in-loop */
}

export async function delay (time: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(() => {
    resolve()
  }, time))
}
