import { MetadataCache, type MetadataIndex } from '@pnpm/cache.metadata'

export { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

export function registryHost (registry?: string): string {
  return new URL(registry ?? 'https://registry.npmjs.org/').host
}

export async function retryLoadFromCache (cacheDir: string, name: string, _type?: string, registry?: string): Promise<MetadataIndex> {
  const dbName = `${registryHost(registry)}/${name}`
  let retry = 0
  /* eslint-disable no-await-in-loop */
  while (true) {
    await delay(500)
    const db = new MetadataCache(cacheDir)
    try {
      const index = db.getIndex(dbName)
      if (index) {
        return index
      }
      if (retry > 2) throw new Error(`No cache entry found for ${dbName}`)
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
